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
    async fn get(
        &self,
        key: &str,
        container: Option<&str>,
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
        key: &str,
        container: Option<&str>,
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
        key: &str,
        container: Option<&str>,
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
        key: &str,
        container: Option<&str>,
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
    use super::{LocalDiskCacheBackend, *};
    use bytes::Bytes;

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn backend() -> LocalDiskCacheBackend {
        LocalDiskCacheBackend::new(temp_dir().path(), "default")
    }

    #[tokio::test]
    async fn test_get() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend.save(key, None, value.clone()).await.unwrap();

        let result = backend.get(key, None).await.unwrap();

        assert_eq!(result, Some(value));
    }

    #[tokio::test]
    async fn test_get_container() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend
            .save(key, Some("container"), value.clone())
            .await
            .unwrap();

        let result = backend.get(key, Some("container")).await.unwrap();

        assert_eq!(result, Some(value));
    }

    #[tokio::test]
    async fn test_delete() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend.save(key, None, value.clone()).await.unwrap();

        let result = backend.delete(key, None).await.unwrap();

        assert_eq!(result, ());

        let result = backend.get(key, None).await.unwrap();

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_delete_container() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend
            .save(key, Some("container"), value.clone())
            .await
            .unwrap();

        let result = backend.delete(key, Some("container")).await.unwrap();

        assert_eq!(result, ());

        let result = backend.get(key, Some("container")).await.unwrap();

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_size() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend.save(key, None, value.clone()).await.unwrap();

        let result = backend.size(key, None).await.unwrap();

        assert_eq!(result, Some(ByteSize::b(value.len() as u64)));
    }

    #[tokio::test]
    async fn test_size_container() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend
            .save(key, Some("container"), value.clone())
            .await
            .unwrap();

        let result = backend.size(key, Some("container")).await.unwrap();

        assert_eq!(result, Some(ByteSize::b(value.len() as u64)));
    }

    #[tokio::test]
    async fn test_list() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend.save(key, None, value.clone()).await.unwrap();

        let result = backend.list(None).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, key);
        assert_eq!(result[0].size, ByteSize::b(value.len() as u64));
    }

    #[tokio::test]
    async fn test_list_container() {
        let backend = backend();

        let key = "test";
        let value = Bytes::from("test");

        backend
            .save(key, Some("container"), value.clone())
            .await
            .unwrap();

        let result = backend.list(Some("container")).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, key);
        assert_eq!(result[0].size, ByteSize::b(value.len() as u64));
    }
}
