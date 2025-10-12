use std::num::NonZeroUsize;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use bytesize::ByteSize;
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;

use crate::{
    BoxStream, ListItem, PageOptions, RemoteCacheStorageBackend, error::Error,
};

#[derive(Debug)]
pub struct LruCached<T: RemoteCacheStorageBackend> {
    inner: T,
    cached: Mutex<lru::LruCache<(String, String), Bytes>>,
}

impl<T: RemoteCacheStorageBackend> LruCached<T> {
    pub fn new(inner: T, capacity: NonZeroUsize) -> Self {
        Self {
            inner,
            cached: Mutex::new(lru::LruCache::new(capacity)),
        }
    }
}

impl<T: RemoteCacheStorageBackend> LruCached<T> {
    fn container<'s>(&'s self, container: Option<&'s str>) -> &'s str {
        container.unwrap_or(self.inner.default_container())
    }
}

#[async_trait]
impl<T> RemoteCacheStorageBackend for LruCached<T>
where
    T: RemoteCacheStorageBackend + Send + Sync,
{
    fn default_container(&self) -> &str {
        self.inner.default_container()
    }

    async fn get(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        let container_name = self.container(container);
        if let Some(cached) = self
            .cached
            .lock()
            .await
            .get(&(container_name.to_string(), key.to_string()))
        {
            return Ok(Some(cached.clone()));
        }

        self.inner.get(container, key).await
    }

    async fn exists(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<bool, Error> {
        let container_name = self.container(container);
        if self
            .cached
            .lock()
            .await
            .get(&(container_name.to_string(), key.to_string()))
            .is_some()
        {
            return Ok(true);
        }

        self.inner.exists(container, key).await
    }

    async fn get_stream(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<BoxStream<Bytes>>, Error> {
        let container_name = self.container(container);
        if let Some(cached) = self
            .cached
            .lock()
            .await
            .get(&(container_name.to_string(), key.to_string()))
        {
            let cloned = cached.clone();
            return Ok(Some(Box::pin(tokio_stream::once(cloned))));
        }

        self.inner.get_stream(container, key).await
    }

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<Vec<ListItem>, Error> {
        self.inner.list(container).await
    }

    async fn paged_list(
        &self,
        container: Option<&str>,
        query: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        self.inner.paged_list(container, query).await
    }

    async fn save(
        &self,
        container: Option<&str>,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error> {
        self.cached.lock().await.put(
            (self.container(container).to_string(), key.to_string()),
            value.clone(),
        );

        self.inner.save(container, key, value).await
    }

    async fn save_stream(
        &self,
        container: Option<&str>,
        key: &str,
        value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
        let collected = value.collect::<Vec<Bytes>>().await;
        let mut combined = BytesMut::new();

        for chunk in collected {
            combined.extend_from_slice(&chunk);
        }

        let combined = combined.freeze();

        self.cached.lock().await.put(
            (self.container(container).to_string(), key.to_string()),
            combined.clone(),
        );

        self.inner.save(container, key, combined).await
    }

    async fn delete(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<(), Error> {
        self.cached
            .lock()
            .await
            .pop(&(self.container(container).to_string(), key.to_string()));

        self.inner.delete(container, key).await
    }

    async fn size(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let container_name = self.container(container);
        if let Some(cached) = self
            .cached
            .lock()
            .await
            .get(&(container_name.to_string(), key.to_string()))
        {
            return Ok(Some(ByteSize::b(cached.len() as u64)));
        }

        self.inner.size(container, key).await
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::LruCached;
    use crate::decl_remote_cache_storage_backend_tests;
    use crate::impls::InMemoryBackend;

    decl_remote_cache_storage_backend_tests!(LruCached::new(
        InMemoryBackend::new("default"),
        NonZeroUsize::new(100).unwrap()
    ));
}
