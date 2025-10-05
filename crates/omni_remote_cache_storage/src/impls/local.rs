use std::path::PathBuf;

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use tokio::{
    fs::{File, OpenOptions},
    io::AsyncWriteExt as _,
    task::JoinSet,
};
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

use crate::{
    BoxStream, ListItem, PageOptions, RemoteCacheStorageBackend, error::Error,
};

#[derive(Debug, new)]
pub struct LocalDiskCacheBackend {
    #[new(into)]
    root: PathBuf,
    #[new(into)]
    default_container: String,
}

impl LocalDiskCacheBackend {
    fn path(&self, key: &str, container: Option<&str>) -> PathBuf {
        if let Some(container) = container {
            self.root.join(container).join(key)
        } else {
            self.root.join(&self.default_container).join(key)
        }
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for LocalDiskCacheBackend {
    fn default_container(&self) -> &str {
        &self.default_container
    }

    async fn get(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<bytes::Bytes>, Error> {
        let path = self.path(key, container);

        if tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            let bytes = tokio::fs::read(&path).await.map_err(Error::custom)?;

            Ok(Some(Bytes::from_owner(bytes)))
        } else {
            Ok(None)
        }
    }

    async fn get_stream(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<BoxStream<Bytes>>, Error> {
        let path = self.path(key, container);

        if tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            let stream = ReaderStream::new(
                File::open(path).await.map_err(Error::custom)?,
            )
            .filter_map(|x| match x {
                Ok(b) => Some(b),
                Err(e) => {
                    trace::error!("error reading file: {}", e);
                    None
                }
            });

            Ok(Some(Box::pin(stream) as BoxStream<Bytes>))
        } else {
            Ok(None)
        }
    }

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<Vec<ListItem>, Error> {
        let dir = self.path("", container);

        let mut read_dir =
            tokio::fs::read_dir(&dir).await.map_err(Error::custom)?;

        let mut futs = JoinSet::new();

        while let Some(entry) =
            read_dir.next_entry().await.map_err(Error::custom)?
        {
            let path = entry.path();
            let key = path
                .strip_prefix(&dir)
                .map_err(Error::custom)?
                .to_str()
                .ok_or_else(|| Error::custom(eyre::eyre!("invalid path")))?
                .to_string();
            futs.spawn(async move {
                let metadata =
                    tokio::fs::metadata(&path).await.map_err(Error::custom)?;
                Ok::<_, Error>(ListItem {
                    key,
                    size: ByteSize::b(metadata.len()),
                })
            });
        }

        let results = futs.join_all().await;
        let mut items = Vec::with_capacity(results.len());

        for result in results {
            items.push(result.map_err(Error::custom)?);
        }

        Ok(items)
    }

    async fn paged_list(
        &self,
        container: Option<&str>,
        query: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        let all = self.list(container).await?;

        let position = all
            .iter()
            .position(|item| {
                if let Some(after_key) = query.after_key.as_ref() {
                    item.key == *after_key
                } else {
                    true
                }
            })
            .unwrap_or(0);

        Ok(all
            .into_iter()
            .skip(position)
            .take(query.per_page.unwrap_or(100) as usize)
            .collect::<Vec<_>>())
    }

    async fn save(
        &self,
        container: Option<&str>,
        key: &str,
        value: bytes::Bytes,
    ) -> Result<(), Error> {
        let path = self.path(key, container);

        if !tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            tokio::fs::create_dir_all(
                &path.parent().expect("should have parent"),
            )
            .await
            .map_err(Error::custom)?;
        }

        tokio::fs::write(&path, value.as_ref())
            .await
            .map_err(Error::custom)?;

        Ok(())
    }

    async fn save_stream(
        &self,
        container: Option<&str>,
        key: &str,
        mut value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
        let path = self.path(key, container);

        if !tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            tokio::fs::create_dir_all(
                &path.parent().expect("should have parent"),
            )
            .await
            .map_err(Error::custom)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .await
            .map_err(Error::custom)?;

        while let Some(chunk) = value.next().await {
            file.write_all(&chunk).await.map_err(Error::custom)?;
        }
        Ok(())
    }

    async fn delete(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<(), Error> {
        let path = self.path(key, container);

        if !tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            return Ok(());
        }

        tokio::fs::remove_file(&path).await.map_err(Error::custom)?;

        Ok(())
    }

    async fn size(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let path = self.path(key, container);

        if !tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            return Ok(None);
        }

        let metadata =
            tokio::fs::metadata(&path).await.map_err(Error::custom)?;
        Ok(Some(ByteSize::b(metadata.len())))
    }
}

#[cfg(test)]
mod tests {
    use super::LocalDiskCacheBackend;
    use crate::decl_remote_cache_storage_backend_tests;
    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    decl_remote_cache_storage_backend_tests!(LocalDiskCacheBackend::new(
        temp_dir().path(),
        "default"
    ));
}
