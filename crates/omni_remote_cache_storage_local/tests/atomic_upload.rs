use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt;
use omni_remote_cache_storage_core::{Container, RemoteCacheStorageBackend};
use omni_remote_cache_storage_local::LocalDiskCacheBackend;

#[tokio::test]
async fn publication_creates_and_syncs_a_missing_backend_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("new-parent").join("cache-root");
    let backend = LocalDiskCacheBackend::new(&root);
    let container = Container::parse("ns/ci").unwrap();
    backend
        .save(&container, "d", Bytes::from("complete"))
        .await
        .unwrap();
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("complete"))
    );
    assert!(root.join("ns/ci/d").is_file());
}

#[tokio::test]
async fn replacement_publishes_only_complete_bytes_and_preserves_open_readers()
{
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalDiskCacheBackend::new(root.path()));
    let container = Container::parse("ns/ci").unwrap();
    backend
        .save(&container, "d", Bytes::from("old complete"))
        .await
        .unwrap();
    let mut reader =
        backend.get_stream(&container, "d").await.unwrap().unwrap();
    let (started, waiting) = tokio::sync::oneshot::channel();
    let (release, gate) = tokio::sync::oneshot::channel();
    let stream =
        futures::stream::once(std::future::ready(Ok(Bytes::from("new "))))
            .chain(futures::stream::once(async move {
                started.send(()).unwrap();
                gate.await.unwrap();
                Ok(Bytes::from("complete"))
            }));
    let writer = backend.clone();
    let write_container = container.clone();
    let upload = tokio::spawn(async move {
        writer
            .save_stream(&write_container, "d", Box::pin(stream))
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("old complete"))
    );
    release.send(()).unwrap();
    upload.await.unwrap().unwrap();
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("new complete"))
    );
    let mut old = Vec::new();
    while let Some(chunk) = reader.next().await {
        old.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(old, b"old complete");
    assert!(
        std::fs::read_dir(root.path()).unwrap().all(|entry| entry
            .unwrap()
            .file_type()
            .unwrap()
            .is_dir())
    );
}

#[tokio::test]
async fn failed_and_cancelled_uploads_remove_staging_files() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalDiskCacheBackend::new(root.path()));
    let container = Container::parse("ns/ci").unwrap();
    let stream = futures::stream::iter([
        Ok(Bytes::from("partial")),
        Err(omni_remote_cache_storage_core::error::Error::custom(
            std::io::Error::other("interrupted"),
        )),
    ]);
    assert!(
        backend
            .save_stream(&container, "d", Box::pin(stream))
            .await
            .is_err()
    );
    assert!(backend.get(&container, "d").await.unwrap().is_none());

    let (started, waiting) = tokio::sync::oneshot::channel();
    let stream =
        futures::stream::once(std::future::ready(Ok(Bytes::from("partial"))))
            .chain(futures::stream::once(async move {
                started.send(()).unwrap();
                std::future::pending().await
            }));
    let writer = backend.clone();
    let write_container = container.clone();
    let upload = tokio::spawn(async move {
        writer
            .save_stream(&write_container, "d", Box::pin(stream))
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    upload.abort();
    assert!(upload.await.unwrap_err().is_cancelled());
    assert!(backend.get(&container, "d").await.unwrap().is_none());
    assert!(
        std::fs::read_dir(root.path()).unwrap().all(|entry| entry
            .unwrap()
            .file_type()
            .unwrap()
            .is_dir())
    );
}

#[tokio::test]
async fn concurrent_new_uploads_never_expose_partial_data_or_damage_a_winner() {
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalDiskCacheBackend::new(root.path()));
    let container = Container::parse("ns/ci").unwrap();
    let mut writers = Vec::new();
    for _ in 0..2 {
        let (started, waiting) = tokio::sync::oneshot::channel();
        let (release, gate) = tokio::sync::oneshot::channel();
        let stream = futures::stream::once(std::future::ready(Ok(
            Bytes::from("first "),
        )))
        .chain(futures::stream::once(async move {
            started.send(()).unwrap();
            gate.await.unwrap()
        }));
        let writer = backend.clone();
        let write_container = container.clone();
        let task = tokio::spawn(async move {
            writer
                .save_stream(&write_container, "d", Box::pin(stream))
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(10), waiting)
            .await
            .unwrap()
            .unwrap();
        writers.push((release, task));
    }
    assert!(backend.get(&container, "d").await.unwrap().is_none());
    assert!(backend.list(&container).await.unwrap().is_empty());
    let (release, first) = writers.remove(0);
    release.send(Ok(Bytes::from("complete"))).unwrap();
    first.await.unwrap().unwrap();
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("first complete"))
    );
    let (release, second) = writers.remove(0);
    release
        .send(Err(omni_remote_cache_storage_core::error::Error::custom(
            std::io::Error::other("interrupted"),
        )))
        .unwrap();
    assert!(second.await.unwrap().is_err());
    assert_eq!(
        backend.get(&container, "d").await.unwrap(),
        Some(Bytes::from("first complete"))
    );
}
