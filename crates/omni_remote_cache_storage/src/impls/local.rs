use std::path::PathBuf;

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use tokio::task::JoinSet;

use crate::{ListItem, RemoteCacheStorageBackend, error::Error};

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
