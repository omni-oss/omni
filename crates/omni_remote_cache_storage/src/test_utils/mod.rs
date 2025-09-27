#[macro_export]
macro_rules! decl_remote_cache_storage_backend_tests {
    ($default:expr) => {
        use crate::RemoteCacheStorageBackend;
        use bytes::Bytes;
        use bytesize::ByteSize;

        async fn backend() -> impl RemoteCacheStorageBackend + Send + Sync {
            let backend = $default;

            backend
                .save(None, "key1", Bytes::from("value1"))
                .await
                .unwrap();
            backend
                .save(None, "key2", Bytes::from("value2"))
                .await
                .unwrap();

            backend
                .save(Some("container1"), "key1", Bytes::from("value2"))
                .await
                .unwrap();
            backend
                .save(Some("container1"), "key2", Bytes::from("value2"))
                .await
                .unwrap();

            backend
        }

        #[tokio::test]
        async fn test_get() {
            let backend = backend().await;

            let value = backend
                .get(None, "key1")
                .await
                .expect("should have no error")
                .expect("should have data");

            assert_eq!(value, Bytes::from("value1"));
        }

        #[tokio::test]
        async fn test_get_container() {
            let backend = backend().await;

            let value = backend
                .get(Some("container1"), "key1")
                .await
                .expect("should have no error")
                .expect("should have data");

            assert_eq!(value, Bytes::from("value2"));
        }

        #[tokio::test]
        async fn test_list() {
            let backend = backend().await;

            let mut list = backend.list(None).await.unwrap();
            list.sort_by_key(|item| item.key.clone());

            assert_eq!(list.len(), 2);
            assert_eq!(list[0].key, "key1");
            assert_eq!(list[0].size, ByteSize::b(6));
            assert_eq!(list[1].key, "key2");
            assert_eq!(list[1].size, ByteSize::b(6));
        }

        #[tokio::test]
        async fn test_list_container() {
            let backend = backend().await;

            let mut list = backend
                .list(Some("container1"))
                .await
                .expect("should have no error");

            list.sort_by_key(|item| item.key.clone());

            assert_eq!(list.len(), 2);
            assert_eq!(list[0].key, "key1");
            assert_eq!(list[0].size, ByteSize::b(6));
            assert_eq!(list[1].key, "key2");
            assert_eq!(list[1].size, ByteSize::b(6));
        }

        #[tokio::test]
        async fn test_save() {
            let backend = backend().await;

            backend
                .save(None, "key3", Bytes::from("value3"))
                .await
                .unwrap();
        }

        #[tokio::test]
        async fn test_save_container() {
            let backend = backend().await;

            backend
                .save(Some("container2"), "key1", Bytes::from("value3"))
                .await
                .unwrap();

            let list = backend.list(Some("container2")).await.unwrap();
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].key, "key1");
            assert_eq!(list[0].size, ByteSize::b(6));
        }

        #[tokio::test]
        async fn test_delete() {
            let backend = backend().await;

            backend.delete(None, "key1").await.unwrap();
            backend.delete(None, "key2").await.unwrap();

            let list_default = backend.list(None).await.unwrap();
            assert_eq!(list_default.len(), 0);
        }

        #[tokio::test]
        async fn test_delete_container() {
            let backend = backend().await;

            backend.delete(Some("container1"), "key1").await.unwrap();
            backend.delete(Some("container1"), "key2").await.unwrap();

            let list = backend.list(Some("container1")).await.unwrap();
            assert_eq!(list.len(), 0);
        }
    };
}
