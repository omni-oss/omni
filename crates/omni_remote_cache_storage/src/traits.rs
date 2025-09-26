use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;

use crate::{ListItem, error::Error};

#[async_trait]
pub trait RemoteCacheStorageBackend {
    async fn get(
        &self,
        key: &str,
        container: Option<&str>,
    ) -> Result<Option<Bytes>, Error>;

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<Vec<ListItem>, Error>;

    async fn save(
        &self,
        key: &str,
        container: Option<&str>,
        value: Bytes,
    ) -> Result<(), Error>;

    async fn delete(
        &self,
        key: &str,
        container: Option<&str>,
    ) -> Result<(), Error>;

    async fn size(
        &self,
        key: &str,
        container: Option<&str>,
    ) -> Result<Option<ByteSize>, Error>;
}

#[async_trait]
pub trait RemoteCacheStorageBackendExt: RemoteCacheStorageBackend {
    async fn get_default(&self, key: &str) -> Result<Option<Bytes>, Error> {
        self.get(key, None).await
    }

    async fn list_default(&self) -> Result<Vec<ListItem>, Error> {
        self.list(None).await
    }

    async fn save_default(&self, key: &str, value: Bytes) -> Result<(), Error> {
        self.save(key, None, value).await
    }

    async fn delete_default(&self, key: &str) -> Result<(), Error> {
        self.delete(key, None).await
    }

    async fn size_default(&self, key: &str) -> Result<Option<ByteSize>, Error> {
        self.size(key, None).await
    }
}

impl<T: RemoteCacheStorageBackend> RemoteCacheStorageBackendExt for T {}
