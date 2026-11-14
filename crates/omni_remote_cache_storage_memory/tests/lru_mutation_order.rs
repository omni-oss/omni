use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use omni_remote_cache_storage_core::{
    Container, ListItem, PageOptions, ReadStream, RemoteCacheStorageBackend,
    WriteStream, decorators::LruCached, error::Error,
};
use omni_remote_cache_storage_memory::InMemoryBackend;
use tokio::sync::Notify;

struct PausedStreamBackend {
    inner: Arc<InMemoryBackend>,
    committed: Arc<Notify>,
    resume: Arc<Notify>,
}

#[async_trait]
impl RemoteCacheStorageBackend for PausedStreamBackend {
    async fn get(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<Bytes>, Error> {
        self.inner.get(c, k).await
    }
    async fn exists(&self, c: &Container, k: &str) -> Result<bool, Error> {
        self.inner.exists(c, k).await
    }
    async fn get_stream(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<ReadStream>, Error> {
        self.inner.get_stream(c, k).await
    }
    async fn list(&self, c: &Container) -> Result<Vec<ListItem>, Error> {
        self.inner.list(c).await
    }
    async fn paged_list(
        &self,
        c: &Container,
        p: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        self.inner.paged_list(c, p).await
    }
    async fn save(
        &self,
        c: &Container,
        k: &str,
        b: Bytes,
    ) -> Result<(), Error> {
        self.inner.save(c, k, b).await
    }
    async fn save_stream(
        &self,
        c: &Container,
        k: &str,
        s: WriteStream,
    ) -> Result<(), Error> {
        self.inner.save_stream(c, k, s).await?;
        self.committed.notify_one();
        self.resume.notified().await;
        Ok(())
    }
    async fn delete(&self, c: &Container, k: &str) -> Result<(), Error> {
        self.inner.delete(c, k).await
    }
    async fn size(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<ByteSize>, Error> {
        self.inner.size(c, k).await
    }
}

#[tokio::test]
async fn obsolete_stream_cannot_repopulate_after_a_byte_write_or_delete() {
    for delete in [false, true] {
        let committed = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        let inner = Arc::new(InMemoryBackend::new());
        let cache = Arc::new(LruCached::new(
            PausedStreamBackend {
                inner: inner.clone(),
                committed: committed.clone(),
                resume: resume.clone(),
            },
            NonZeroUsize::new(10).unwrap(),
        ));
        let container = Container::parse("ns/ci").unwrap();
        let writer = cache.clone();
        let c = container.clone();
        let stream = tokio::spawn(async move {
            writer
                .save_stream(
                    &c,
                    "d",
                    Box::pin(futures::stream::iter([Ok(Bytes::from("old"))])),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), committed.notified())
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(5),
            cache.save(&container, "unrelated", Bytes::from("independent")),
        )
        .await
        .expect("unrelated keys must not wait for a stalled stream")
        .unwrap();

        let mutation = async {
            if delete {
                cache.delete(&container, "d").await
            } else {
                cache.save(&container, "d", Bytes::from("new")).await
            }
        };
        tokio::pin!(mutation);
        // Poll while the stream is paused *after* backend publication. Without
        // mutation coordination this completes, allowing stale LRU repopulation.
        assert!(futures::poll!(&mut mutation).is_pending());
        resume.notify_one();
        tokio::time::timeout(Duration::from_secs(5), stream)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), mutation)
            .await
            .unwrap()
            .unwrap();
        let expected = if delete {
            None
        } else {
            Some(Bytes::from("new"))
        };
        assert_eq!(cache.get(&container, "d").await.unwrap(), expected);
        assert_eq!(inner.get(&container, "d").await.unwrap(), expected);
        assert_eq!(cache.exists(&container, "d").await.unwrap(), !delete);
    }
}

#[tokio::test]
async fn uncontested_stream_still_populates_the_lru() {
    // Verify caching through observable behavior: changing the underlying
    // backend directly leaves the successfully captured stream available.
    let committed = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    resume.notify_one();
    let inner = Arc::new(InMemoryBackend::new());
    let cache = LruCached::new(
        PausedStreamBackend {
            inner: inner.clone(),
            committed,
            resume,
        },
        NonZeroUsize::new(10).unwrap(),
    );
    let container = Container::parse("ns/ci").unwrap();
    cache
        .save_stream(
            &container,
            "d",
            Box::pin(futures::stream::iter([Ok(Bytes::from("old"))])),
        )
        .await
        .unwrap();
    inner.delete(&container, "d").await.unwrap();
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        Some(Bytes::from("old"))
    );
}
