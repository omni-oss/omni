#![allow(clippy::redundant_field_names)]

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

use omni_remote_cache_storage_core::{
    BoxStream, Container, ListItem, PageOptions, ReadStream,
    RemoteCacheStorageBackend, error::Error,
};

#[derive(Debug, new)]
pub struct LocalDiskCacheBackend {
    #[new(into)]
    root: PathBuf,
}

impl LocalDiskCacheBackend {
    fn path(&self, key: &str, container: &Container) -> Result<PathBuf, Error> {
        if key.is_empty()
            || key.contains(['/', '\\'])
            || key == "."
            || key == ".."
            || key.chars().any(|c| c.is_control())
        {
            return Err(Error::custom(eyre::eyre!("invalid key {key:?}")));
        }
        Ok(self.dir(container).join(key))
    }

    fn dir(&self, container: &Container) -> PathBuf {
        self.root.join(container.as_str())
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for LocalDiskCacheBackend {
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<bytes::Bytes>, Error> {
        let path = self.path(key, container)?;

        if tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            let bytes = tokio::fs::read(&path).await.map_err(Error::custom)?;

            Ok(Some(Bytes::from_owner(bytes)))
        } else {
            Ok(None)
        }
    }

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, Error> {
        let path = self.path(key, container)?;
        Ok(tokio::fs::try_exists(&path).await.map_err(Error::custom)?)
    }

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error> {
        let path = self.path(key, container)?;

        if tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            let stream = ReaderStream::new(
                File::open(path).await.map_err(Error::custom)?,
            )
            .map(|r| r.map_err(Error::custom));

            Ok(Some(Box::pin(stream) as ReadStream))
        } else {
            Ok(None)
        }
    }

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        let dir = self.dir(container);

        if !tokio::fs::try_exists(&dir).await.map_err(Error::custom)? {
            return Ok(Vec::new());
        }

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
        container: &Container,
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
        container: &Container,
        key: &str,
        value: bytes::Bytes,
    ) -> Result<(), Error> {
        let path = self.path(key, container)?;

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
        container: &Container,
        key: &str,
        mut value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
        let path = self.path(key, container)?;

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
        container: &Container,
        key: &str,
    ) -> Result<(), Error> {
        let path = self.path(key, container)?;

        if !tokio::fs::try_exists(&path).await.map_err(Error::custom)? {
            return Ok(());
        }

        tokio::fs::remove_file(&path).await.map_err(Error::custom)?;

        Ok(())
    }

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let path = self.path(key, container)?;

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
    use omni_remote_cache_storage_core::decl_remote_cache_storage_backend_tests;
    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    decl_remote_cache_storage_backend_tests!(LocalDiskCacheBackend::new(
        temp_dir().path()
    ));

    #[tokio::test]
    async fn rejects_keys_that_escape_the_container() {
        use bytes::Bytes;
        use omni_remote_cache_storage_core::{
            Container, RemoteCacheStorageBackend,
        };
        let dir = temp_dir();
        let b = LocalDiskCacheBackend::new(dir.path());
        let c = Container::parse("ns/ci").unwrap();
        for key in ["../x", "/abs", "a/b", "..", ""] {
            assert!(b.save(&c, key, Bytes::from("x")).await.is_err(), "{key}");
            assert!(b.get(&c, key).await.is_err(), "{key}");
        }
    }
}
