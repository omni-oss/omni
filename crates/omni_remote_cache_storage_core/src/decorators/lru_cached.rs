use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use bytesize::ByteSize;
use futures::StreamExt as _;

use crate::{
    ByteRange, Container, ContainerPrefix, ListItem, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream, error::Error,
};

const DEFAULT_MAX_CACHEABLE_SIZE: ByteSize = ByteSize::mib(8);

type Cache = lru::LruCache<(String, String), Bytes>;
type MutationLocks = HashMap<(String, String), Weak<futures::lock::Mutex<()>>>;

#[derive(Debug)]
/// Native deletes require a Tokio runtime and finish even if the caller cancels.
pub struct LruCached<T: RemoteCacheStorageBackend> {
    inner: Arc<T>,
    cached: Arc<Mutex<Cache>>,
    max_cacheable_size: ByteSize,
    // Weak entries track only live same-key mutations. Never hold this map
    // or the cache mutex across an await.
    mutations: Mutex<MutationLocks>,
    // Bumped around every prefix delete. A streamed write that started before
    // a bump must not cache its bytes, or it could resurrect a purged entry.
    purges: Arc<AtomicU64>,
}

impl<T: RemoteCacheStorageBackend> LruCached<T> {
    pub fn new(inner: T, capacity: NonZeroUsize) -> Self {
        Self {
            inner: Arc::new(inner),
            cached: Arc::new(Mutex::new(lru::LruCache::new(capacity))),
            max_cacheable_size: DEFAULT_MAX_CACHEABLE_SIZE,
            mutations: Mutex::new(HashMap::new()),
            purges: Arc::new(AtomicU64::new(0)),
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

    fn mutation_lock(
        &self,
        container: &Container,
        key: &str,
    ) -> Arc<futures::lock::Mutex<()>> {
        let mut mutations = self
            .mutations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Reclaim completed keys instead of retaining a lock per lifetime key.
        mutations.retain(|_, lock| lock.strong_count() > 0);
        let entry =
            mutations.entry(self.cache_key(container, key)).or_default();
        if let Some(lock) = entry.upgrade() {
            return lock;
        }
        let lock = Arc::new(futures::lock::Mutex::new(()));
        *entry = Arc::downgrade(&lock);
        lock
    }
}

#[async_trait]
impl<T> RemoteCacheStorageBackend for LruCached<T>
where
    T: RemoteCacheStorageBackend + Send + Sync + 'static,
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

    async fn get_range(
        &self,
        container: &Container,
        key: &str,
        range: ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        let cached = self.cache().get(&self.cache_key(container, key)).cloned();
        if let Some(cached) = cached {
            let selected = range.resolve(cached.len() as u64);
            return Ok(Some(
                cached.slice(selected.start as usize..selected.end as usize),
            ));
        }

        self.inner.get_range(container, key, range).await
    }

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        self.inner.list(container).await
    }

    async fn list_prefix(
        &self,
        prefix: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, Error> {
        self.inner.list_prefix(prefix, after, limit).await
    }

    async fn list_children(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        self.inner.list_children(prefix).await
    }

    async fn delete_prefix(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<u64, Error> {
        // Purge around the backend call so a read in between cannot serve an
        // entry that is being deleted, and bump the generation so a streamed
        // write already in flight does not re-cache what the purge removed.
        // Runs detached on native so cancelling the caller cannot leave the
        // cache half-purged.
        let inner = self.inner.clone();
        let cached = self.cached.clone();
        let purges = self.purges.clone();
        let prefix = prefix.clone();
        let purge = async move {
            purge_cached(&cached, &purges, &prefix);
            let result = inner.delete_prefix(&prefix).await;
            purge_cached(&cached, &purges, &prefix);
            result
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::spawn(purge).await.map_err(Error::custom)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            purge.await
        }
    }

    async fn save_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<SaveOutcome, Error> {
        let mutation = self.mutation_lock(container, key);
        let _mutation = mutation.lock().await;
        self.cache().pop(&self.cache_key(container, key));
        let outcome = self.inner.save_if_absent(container, key, value).await?;

        // Invalidate rather than warm: a concurrent delete may have removed
        // the object after the backend accepted it. Cache misses read the backend.
        self.cache().pop(&self.cache_key(container, key));

        Ok(outcome)
    }

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        let mutation = self.mutation_lock(container, key);
        let _mutation = mutation.lock().await;
        self.cache().pop(&self.cache_key(container, key));
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
                let Ok(chunk) = chunk else {
                    *guard = None;
                    return;
                };
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
        let generation = self.purges.load(Ordering::SeqCst);
        let result = self
            .inner
            .save_stream_if_absent(container, key, Box::pin(teed))
            .await;

        let captured = capture
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();

        // The generation is checked while holding the cache lock, so a purge
        // that bumps it afterwards must also take the lock to clear the cache
        // and so removes what is inserted here.
        let mut cache = self.cache();
        match (&result, captured) {
            // Cache the captured bytes only when they are what was stored. On
            // `AlreadyExists` the stored object is a different, earlier entry.
            (Ok(SaveOutcome::Stored), Some(buf))
                if self.purges.load(Ordering::SeqCst) == generation =>
            {
                cache.put(cache_key, buf.freeze());
            }
            _ => {
                cache.pop(&cache_key);
            }
        }
        drop(cache);

        result
    }

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), Error> {
        let mutation = self.mutation_lock(container, key);
        let guard = mutation.lock_owned().await;
        let inner = self.inner.clone();
        let cached = self.cached.clone();
        let cache_key = self.cache_key(container, key);
        let container = container.clone();
        let key = key.to_string();
        let delete = async move {
            // Own the guard through removal completion, even if the caller
            // drops its JoinHandle while filesystem work is still running.
            let _guard = guard;
            cached
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop(&cache_key);
            let result = inner.delete(&container, &key).await;
            cached
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop(&cache_key);
            result
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::spawn(delete).await.map_err(Error::custom)?
        }
        // Workers have no Tokio runtime or detached native filesystem work.
        #[cfg(target_arch = "wasm32")]
        {
            delete.await
        }
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

/// Drops every cached entry whose object path starts with `prefix`.
fn purge_cached(
    cached: &Mutex<Cache>,
    purges: &AtomicU64,
    prefix: &ContainerPrefix,
) {
    purge_cached_then(cached, purges, prefix, || {});
}

// The callback is a deterministic observation point for the lock-boundary test;
// in production it is an inlined no-op.
fn purge_cached_then(
    cached: &Mutex<Cache>,
    purges: &AtomicU64,
    prefix: &ContainerPrefix,
    after_clear: impl FnOnce(),
) {
    let mut cache = cached.lock().unwrap_or_else(PoisonError::into_inner);
    // Both transitions (before and after the backend delete) must be atomic
    // with clearing. Otherwise a writer that began during deletion can insert
    // between the final clear and its generation transition.
    purges.fetch_add(1, Ordering::SeqCst);
    let doomed: Vec<(String, String)> = cache
        .iter()
        .filter(|((container, key), _)| {
            format!("{container}/{key}").starts_with(prefix.as_str())
        })
        .map(|(k, _)| k.clone())
        .collect();
    for key in doomed {
        cache.pop(&key);
    }
    after_clear();
}

#[cfg(test)]
mod purge_tests {
    use super::*;

    #[test]
    fn final_clear_advances_generation_before_releasing_the_cache_lock() {
        let cache = Mutex::new(Cache::new(NonZeroUsize::new(4).unwrap()));
        let purges = AtomicU64::new(1); // writer began while purge was active
        let prefix = ContainerPrefix::parse("isolated/").unwrap();
        let key = ("isolated/t/o/w/e/p/s".into(), "d".into());
        cache
            .lock()
            .unwrap()
            .put(key.clone(), Bytes::from_static(b"old"));
        let writer_generation = purges.load(Ordering::SeqCst);
        purge_cached_then(&cache, &purges, &prefix, || {
            assert!(
                cache.try_lock().is_err(),
                "a writer must still be excluded"
            );
            assert_ne!(
                purges.load(Ordering::SeqCst),
                writer_generation,
                "the final clear must not release with the writer's old generation"
            );
        });
        let mut writer = cache.lock().unwrap();
        if purges.load(Ordering::SeqCst) == writer_generation {
            writer.put(key.clone(), Bytes::from_static(b"resurrected"));
        }
        assert!(writer.get(&key).is_none());
    }
}
