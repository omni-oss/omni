use std::num::NonZeroUsize;

use bytes::Bytes;
use omni_remote_cache_storage_core::{
    Container, RemoteCacheStorageBackend, decorators::LruCached,
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
            .save(&container, key, Bytes::from("rejected"))
            .await
            .is_err()
    );
    assert!(backend.get(&container, key).await.is_err());
    assert!(backend.get_stream(&container, key).await.is_err());
    assert!(backend.exists(&container, key).await.is_err());
    assert!(backend.size(&container, key).await.is_err());
}

#[tokio::test]
async fn successful_write_invalidates_cached_reads_without_warming() {
    let root = tempfile::tempdir().unwrap();
    let backend = LruCached::new(
        LocalDiskCacheBackend::new(root.path()),
        NonZeroUsize::new(10).unwrap(),
    );
    let container = Container::parse("ns/ci").unwrap();
    backend
        .save(&container, "d", Bytes::from("old"))
        .await
        .unwrap();
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("old"))
    );
    backend
        .save(&container, "d", Bytes::from("new"))
        .await
        .unwrap();

    // Model an object removed after backend acceptance, before the next
    // read. No old or newly written bytes should survive in the LRU.
    tokio::fs::remove_file(root.path().join("ns/ci/d"))
        .await
        .unwrap();
    assert_eq!(backend.get(&container, "d").await.unwrap(), None);
    assert!(!backend.exists(&container, "d").await.unwrap());
}
