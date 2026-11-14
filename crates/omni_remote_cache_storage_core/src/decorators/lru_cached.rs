use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use bytesize::ByteSize;
use futures::StreamExt as _;

use crate::{
    BoxStream, Container, ListItem, PageOptions, ReadStream,
    RemoteCacheStorageBackend, error::Error,
};

const DEFAULT_MAX_CACHEABLE_SIZE: ByteSize = ByteSize::mib(8);

type Cache = lru::LruCache<(String, String), Bytes>;

#[derive(Debug)]
pub struct LruCached<T: RemoteCacheStorageBackend> {
    inner: T,
    cached: Mutex<Cache>,
    max_cacheable_size: ByteSize,
}

impl<T: RemoteCacheStorageBackend> LruCached<T> {
    pub fn new(inner: T, capacity: NonZeroUsize) -> Self {
        Self {
            inner,
            cached: Mutex::new(lru::LruCache::new(capacity)),
            max_cacheable_size: DEFAULT_MAX_CACHEABLE_SIZE,
        }
    }

    /// Streamed uploads larger than this are passed through to the inner
    /// backend without being cached, so they are never fully buffered.
    pub fn with_max_cacheable_size(mut self, size: ByteSize) -> Self {
        self.max_cacheable_size = size;
        self
    }
}

impl<T: RemoteCacheStorageBackend> LruCached<T> {
    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cached.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn cache_key(&self, container: &Container, key: &str) -> (String, String) {
        (container.as_str().to_string(), key.to_string())
    }
}

#[async_trait]
impl<T> RemoteCacheStorageBackend for LruCached<T>
where
    T: RemoteCacheStorageBackend + Send + Sync,
{
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        if let Some(cached) = self.cache().get(&self.cache_key(container, key))
        {
            return Ok(Some(cached.clone()));
        }

        self.inner.get(container, key).await
    }

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, Error> {
        if self.cache().get(&self.cache_key(container, key)).is_some() {
            return Ok(true);
        }

        self.inner.exists(container, key).await
    }

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error> {
        let cached = self.cache().get(&self.cache_key(container, key)).cloned();
        if let Some(cached) = cached {
            return Ok(Some(Box::pin(futures::stream::once(
                std::future::ready(Ok(cached)),
            ))));
        }

        self.inner.get_stream(container, key).await
    }

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        self.inner.list(container).await
    }

    async fn paged_list(
        &self,
        container: &Container,
        query: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        self.inner.paged_list(container, query).await
    }

    async fn save(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error> {
        self.cache()
            .put(self.cache_key(container, key), value.clone());

        self.inner.save(container, key, value).await
    }

    async fn save_stream(
        &self,
        container: &Container,
        key: &str,
        value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
        // `None` means the upload exceeded the cacheable size and the
        // captured bytes were dropped.
        let capture = Arc::new(Mutex::new(Some(BytesMut::new())));
        let max = usize::try_from(self.max_cacheable_size.as_u64())
            .unwrap_or(usize::MAX);

        let teed = {
            let capture = capture.clone();
            value.inspect(move |chunk| {
                let mut guard =
                    capture.lock().unwrap_or_else(PoisonError::into_inner);
                let exceeded = match guard.as_mut() {
                    Some(buf) if buf.len() + chunk.len() <= max => {
                        buf.extend_from_slice(chunk);
                        false
                    }
                    Some(_) => true,
                    None => false,
                };
                if exceeded {
                    *guard = None;
                }
            })
        };

        let cache_key = self.cache_key(container, key);
        let result =
            self.inner.save_stream(container, key, Box::pin(teed)).await;

        let captured = capture
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();

        match (&result, captured) {
            (Ok(()), Some(buf)) => {
                self.cache().put(cache_key, buf.freeze());
            }
            _ => {
                self.cache().pop(&cache_key);
            }
        }

        result
    }

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), Error> {
        self.cache().pop(&self.cache_key(container, key));

        self.inner.delete(container, key).await
    }

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        if let Some(cached) = self.cache().get(&self.cache_key(container, key))
        {
            return Ok(Some(ByteSize::b(cached.len() as u64)));
        }

        self.inner.size(container, key).await
    }
}
