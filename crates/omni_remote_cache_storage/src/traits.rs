use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;

use crate::{ListItem, error::Error};

#[async_trait]
pub trait RemoteCacheStorageBackend {
    fn default_container(&self) -> &str;

    async fn get(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<Bytes>, Error>;

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<Vec<ListItem>, Error>;

    async fn save(
        &self,
        container: Option<&str>,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error>;

    async fn delete(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<(), Error>;

    async fn size(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<ByteSize>, Error>;
}

#[async_trait]
pub trait RemoteCacheStorageBackendExt: RemoteCacheStorageBackend {
    async fn get_default(&self, key: &str) -> Result<Option<Bytes>, Error> {
        self.get(None, key).await
    }

    async fn list_default(&self) -> Result<Vec<ListItem>, Error> {
        self.list(None).await
    }

    async fn save_default(&self, key: &str, value: Bytes) -> Result<(), Error> {
        self.save(None, key, value).await
    }

    async fn delete_default(&self, key: &str) -> Result<(), Error> {
        self.delete(None, key).await
    }

    async fn size_default(&self, key: &str) -> Result<Option<ByteSize>, Error> {
        self.size(None, key).await
    }
}

impl<T: RemoteCacheStorageBackend> RemoteCacheStorageBackendExt for T {}
