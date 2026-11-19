use std::num::NonZeroUsize;

use bytes::Bytes;
use omni_remote_cache_storage_core::{
    Container, RemoteCacheStorageBackend, SaveOutcome, decorators::LruCached,
};
use omni_remote_cache_storage_local::LocalDiskCacheBackend;

#[tokio::test]
async fn rejected_write_is_not_cached() {
    let root = tempfile::tempdir().unwrap();
    let backend = LruCached::new(
        LocalDiskCacheBackend::new(root.path()),
        NonZeroUsize::new(10).unwrap(),
    );
    let container = Container::parse("ns/ci").unwrap();
    let key = "../x";
    assert!(
        backend
            .save_if_absent(&container, key, Bytes::from("rejected"))
            .await
            .is_err()
    );
    assert!(backend.get(&container, key).await.is_err());
    assert!(backend.get_stream(&container, key).await.is_err());
    assert!(backend.exists(&container, key).await.is_err());
    assert!(backend.size(&container, key).await.is_err());
}

#[tokio::test]
async fn delete_then_create_never_serves_cached_old_bytes() {
    let root = tempfile::tempdir().unwrap();
    let backend = LruCached::new(
        LocalDiskCacheBackend::new(root.path()),
        NonZeroUsize::new(10).unwrap(),
    );
    let container = Container::parse("ns/ci").unwrap();
    backend
        .save_stream_if_absent(
            &container,
            "d",
            Box::pin(futures::stream::once(std::future::ready(Ok(
                Bytes::from("old"),
            )))),
        )
        .await
        .unwrap();
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("old"))
    );
    backend.delete(&container, "d").await.unwrap();
    assert_eq!(
        backend
            .save_if_absent(&container, "d", Bytes::from("new"))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("new"))
    );
}

#[tokio::test]
async fn a_byte_create_invalidates_and_never_warms_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let backend = LruCached::new(
        LocalDiskCacheBackend::new(root.path()),
        NonZeroUsize::new(10).unwrap(),
    );
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .save_if_absent(&container, "d", Bytes::from("bytes"))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
    // Remove the object behind the decorator's back. If the create had warmed
    // the LRU, the stale bytes would still be served here.
    tokio::fs::remove_file(root.path().join("ns/ci/d"))
        .await
        .unwrap();
    assert_eq!(backend.get(&container, "d").await.unwrap(), None);
    assert!(!backend.exists(&container, "d").await.unwrap());
}
