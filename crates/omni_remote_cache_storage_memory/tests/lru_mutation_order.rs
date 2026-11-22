use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream, decorators::LruCached,
    error::Error,
};
use omni_remote_cache_storage_memory::InMemoryBackend;
use tokio::sync::Notify;

struct PausedStreamBackend {
    inner: Arc<InMemoryBackend>,
    committed: Arc<Notify>,
    resume: Arc<Notify>,
    delete_pause: Option<(Arc<Notify>, Arc<Notify>)>,
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
    async fn get_range(
        &self,
        c: &Container,
        k: &str,
        r: ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        self.inner.get_range(c, k, r).await
    }
    async fn list_prefix(
        &self,
        p: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, Error> {
        self.inner.list_prefix(p, after, limit).await
    }
    async fn list_children(
        &self,
        p: &ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        self.inner.list_children(p).await
    }
    async fn delete_prefix(&self, p: &ContainerPrefix) -> Result<u64, Error> {
        self.inner.delete_prefix(p).await
    }
    async fn save_if_absent(
        &self,
        c: &Container,
        k: &str,
        b: Bytes,
    ) -> Result<SaveOutcome, Error> {
        self.inner.save_if_absent(c, k, b).await
    }
    async fn save_stream_if_absent(
        &self,
        c: &Container,
        k: &str,
        s: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        let outcome = self.inner.save_stream_if_absent(c, k, s).await?;
        self.committed.notify_one();
        self.resume.notified().await;
        Ok(outcome)
    }
    async fn delete(&self, c: &Container, k: &str) -> Result<(), Error> {
        if let Some((submitted, release)) = &self.delete_pause {
            submitted.notify_one();
            release.notified().await;
        }
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
                delete_pause: None,
            },
            NonZeroUsize::new(10).unwrap(),
        ));
        let container = Container::parse("ns/ci").unwrap();
        let writer = cache.clone();
        let c = container.clone();
        let stream = tokio::spawn(async move {
            writer
                .save_stream_if_absent(
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
            cache.save_if_absent(
                &container,
                "unrelated",
                Bytes::from("independent"),
            ),
        )
        .await
        .expect("unrelated keys must not wait for a stalled stream")
        .unwrap();

        let mutation = async {
            if delete {
                cache.delete(&container, "d").await
            } else {
                // Create-only: replacing an entry means deleting it first.
                cache.delete(&container, "d").await?;
                cache
                    .save_if_absent(&container, "d", Bytes::from("new"))
                    .await
                    .map(|_| ())
            }
        };
        // The stream is paused *after* backend publication. The mutation no
        // longer waits for it; the generation check is what stops the stream
        // from re-populating the cache with stale bytes afterwards.
        tokio::time::timeout(Duration::from_secs(5), mutation)
            .await
            .unwrap()
            .unwrap();
        resume.notify_one();
        tokio::time::timeout(Duration::from_secs(5), stream)
            .await
            .unwrap()
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
            delete_pause: None,
        },
        NonZeroUsize::new(10).unwrap(),
    );
    let container = Container::parse("ns/ci").unwrap();
    cache
        .save_stream_if_absent(
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

#[tokio::test]
async fn cancelled_delete_keeps_the_lock_until_backend_removal_finishes() {
    let inner = Arc::new(InMemoryBackend::new());
    let submitted = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    resume.notify_one();
    let cache = Arc::new(LruCached::new(
        PausedStreamBackend {
            inner: inner.clone(),
            committed: Arc::new(Notify::new()),
            resume,
            delete_pause: Some((submitted.clone(), release.clone())),
        },
        NonZeroUsize::new(10).unwrap(),
    ));
    let container = Container::parse("ns/ci").unwrap();
    cache
        .save_if_absent(&container, "d", Bytes::from("old"))
        .await
        .unwrap();
    let deleting = cache.clone();
    let c = container.clone();
    let caller = tokio::spawn(async move { deleting.delete(&c, "d").await });
    tokio::time::timeout(Duration::from_secs(5), submitted.notified())
        .await
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let replacement = cache.save_stream_if_absent(
        &container,
        "d",
        Box::pin(futures::stream::iter([Ok(Bytes::from("new"))])),
    );
    tokio::pin!(replacement);
    assert!(futures::poll!(&mut replacement).is_pending());
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), replacement)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        inner.get(&container, "d").await.unwrap(),
        Some(Bytes::from("new"))
    );
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        Some(Bytes::from("new"))
    );
    inner.delete(&container, "d").await.unwrap();
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        Some(Bytes::from("new")),
        "replacement should be cached"
    );
}

#[tokio::test]
async fn a_byte_write_after_a_published_stream_conflicts_and_is_never_cached() {
    let committed = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let inner = Arc::new(InMemoryBackend::new());
    let cache = Arc::new(LruCached::new(
        PausedStreamBackend {
            inner: inner.clone(),
            committed: committed.clone(),
            resume: resume.clone(),
            delete_pause: None,
        },
        NonZeroUsize::new(10).unwrap(),
    ));
    let container = Container::parse("ns/ci").unwrap();
    let writer = cache.clone();
    let c = container.clone();
    let stream = tokio::spawn(async move {
        writer
            .save_stream_if_absent(
                &c,
                "d",
                Box::pin(futures::stream::iter([Ok(Bytes::from("stream"))])),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), committed.notified())
        .await
        .unwrap();

    // The stream is already published in the backend, so a competing create
    // loses at once instead of waiting for the stalled stream to return.
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(5),
            cache.save_if_absent(&container, "d", Bytes::from("bytes")),
        )
        .await
        .unwrap()
        .unwrap(),
        SaveOutcome::AlreadyExists
    );
    resume.notify_one();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), stream)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        SaveOutcome::Stored
    );
    assert_eq!(
        inner.get(&container, "d").await.unwrap(),
        Some(Bytes::from("stream"))
    );
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        Some(Bytes::from("stream")),
        "the losing writer's bytes must never be served"
    );
}

#[tokio::test]
async fn a_stream_in_flight_during_a_prefix_delete_is_not_cached() {
    let committed = Arc::new(Notify::new());
    let resume = Arc::new(Notify::new());
    let inner = Arc::new(InMemoryBackend::new());
    let cache = Arc::new(LruCached::new(
        PausedStreamBackend {
            inner: inner.clone(),
            committed: committed.clone(),
            resume: resume.clone(),
            delete_pause: None,
        },
        NonZeroUsize::new(10).unwrap(),
    ));
    let container = Container::parse("ns/ci").unwrap();
    let writer = cache.clone();
    let c = container.clone();
    let stream = tokio::spawn(async move {
        writer
            .save_stream_if_absent(
                &c,
                "d",
                Box::pin(futures::stream::iter([Ok(Bytes::from("old"))])),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), committed.notified())
        .await
        .unwrap();
    // The stream is committed in the backend but has not reached the cache yet.
    let removed = cache.delete_prefix(&container.prefix()).await.unwrap();
    assert_eq!(removed, 1);
    resume.notify_one();
    tokio::time::timeout(Duration::from_secs(5), stream)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(inner.get(&container, "d").await.unwrap(), None);
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        None,
        "a purged entry must not be resurrected by an in-flight stream"
    );
}

#[tokio::test]
async fn get_range_is_served_from_a_cached_streamed_entry() {
    let inner = Arc::new(InMemoryBackend::new());
    let resume = Arc::new(Notify::new());
    resume.notify_one();
    let cache = LruCached::new(
        PausedStreamBackend {
            inner: inner.clone(),
            committed: Arc::new(Notify::new()),
            resume,
            delete_pause: None,
        },
        NonZeroUsize::new(10).unwrap(),
    );
    let container = Container::parse("ns/ci").unwrap();
    cache
        .save_stream_if_absent(
            &container,
            "d",
            Box::pin(futures::stream::iter([Ok(Bytes::from("abcdef"))])),
        )
        .await
        .unwrap();
    // Remove it behind the cache's back: only the cache can answer now.
    inner.delete(&container, "d").await.unwrap();
    assert_eq!(
        cache
            .get_range(&container, "d", ByteRange::Suffix(2))
            .await
            .unwrap(),
        Some(Bytes::from("ef"))
    );
    assert_eq!(
        cache
            .get_range(&container, "d", ByteRange::Bounded(1, 3))
            .await
            .unwrap(),
        Some(Bytes::from("bc"))
    );
}

/// A cache whose backend publishes streams without pausing, plus the backend
/// itself so tests can act behind the cache's back.
fn open_cache() -> (Arc<LruCached<PausedStreamBackend>>, Arc<InMemoryBackend>) {
    let inner = Arc::new(InMemoryBackend::new());
    let resume = Arc::new(Notify::new());
    resume.notify_one();
    let cache = Arc::new(LruCached::new(
        PausedStreamBackend {
            inner: inner.clone(),
            committed: Arc::new(Notify::new()),
            resume,
            delete_pause: None,
        },
        NonZeroUsize::new(10).unwrap(),
    ));
    (cache, inner)
}

/// An upload whose body is still arriving: send chunks through the returned
/// sender and drop it to finish the body.
fn trickling_upload() -> (
    futures::channel::mpsc::Sender<Result<Bytes, Error>>,
    WriteStream,
) {
    let (tx, rx) = futures::channel::mpsc::channel(4);
    (tx, Box::pin(rx))
}

#[tokio::test]
async fn a_trickling_upload_does_not_block_a_delete_or_another_write() {
    let (cache, inner) = open_cache();
    let container = Container::parse("ns/ci").unwrap();
    let (mut tx, body) = trickling_upload();
    let writer = cache.clone();
    let c = container.clone();
    let upload = tokio::spawn(async move {
        writer.save_stream_if_absent(&c, "d", body).await
    });
    tx.try_send(Ok(Bytes::from("slow"))).unwrap();
    tokio::task::yield_now().await;

    tokio::time::timeout(Duration::from_secs(5), cache.delete(&container, "d"))
        .await
        .expect("a delete must not wait for a trickling upload")
        .unwrap();
    // Another writer of the same key is not held either, and it wins: the
    // trickling upload only decides at its end, and create-only still holds.
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(5),
            cache.save_if_absent(&container, "d", Bytes::from("fast")),
        )
        .await
        .expect("a write must not wait for a trickling upload")
        .unwrap(),
        SaveOutcome::Stored
    );

    drop(tx);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), upload)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        SaveOutcome::AlreadyExists
    );
    assert_eq!(
        inner.get(&container, "d").await.unwrap(),
        Some(Bytes::from("fast"))
    );
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        Some(Bytes::from("fast")),
        "the losing upload's bytes must never be served"
    );
}

#[tokio::test]
async fn an_upload_that_overlaps_a_delete_is_not_cached() {
    let (cache, inner) = open_cache();
    let container = Container::parse("ns/ci").unwrap();
    let (mut tx, body) = trickling_upload();
    let writer = cache.clone();
    let c = container.clone();
    let upload = tokio::spawn(async move {
        writer.save_stream_if_absent(&c, "d", body).await
    });
    tx.try_send(Ok(Bytes::from("late"))).unwrap();
    tokio::task::yield_now().await;

    cache.delete(&container, "d").await.unwrap();
    drop(tx);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), upload)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        SaveOutcome::Stored
    );
    // The object was published after the delete, so it is the live entry.
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        Some(Bytes::from("late"))
    );
    // But it must not have been cached: removing it behind the cache's back
    // is observed, which a stale cache insert would hide.
    inner.delete(&container, "d").await.unwrap();
    assert_eq!(
        cache.get(&container, "d").await.unwrap(),
        None,
        "an upload that overlapped a delete must not repopulate the cache"
    );
}
