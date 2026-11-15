use bytes::Bytes;
use omni_remote_cache_storage_core::{
    Container, LIST_PAGE_SIZE, RemoteCacheStorageBackend,
};
use omni_remote_cache_storage_memory::InMemoryBackend;

/// `list` is one provided method that every backend shares, so one fast backend
/// proves that it follows every page rather than stopping at the first.
#[tokio::test]
async fn list_follows_every_page() {
    let backend = InMemoryBackend::new();
    let container = Container::parse("many").unwrap();
    let total = (LIST_PAGE_SIZE as usize) * 2 + 5;
    for i in 0..total {
        backend
            .save_if_absent(&container, &format!("k{i:05}"), Bytes::from("x"))
            .await
            .unwrap();
    }
    let all = backend.list(&container).await.unwrap();
    assert_eq!(all.len(), total, "list must follow every page");
    assert!(all.windows(2).all(|w| w[0].key < w[1].key), "in key order");
}
