#[macro_export]
macro_rules! decl_remote_cache_storage_backend_tests {
    ($default:expr) => {
        use bytes::{Bytes, BytesMut};
        use bytesize::ByteSize;
        use futures::StreamExt;
        use $crate::RemoteCacheStorageBackend;

        fn c(name: &str) -> $crate::Container {
            $crate::Container::parse(name).unwrap()
        }

        async fn backend() -> impl RemoteCacheStorageBackend + Send + Sync {
            let backend = $default;

            backend
                .save(&c("default"), "key1", Bytes::from("value1"))
                .await
                .unwrap();
            backend
                .save(&c("default"), "key2", Bytes::from("value2"))
                .await
                .unwrap();

            backend
                .save(&c("container1"), "key1", Bytes::from("value1"))
                .await
                .unwrap();
            backend
                .save(&c("container1"), "key2", Bytes::from("value2"))
                .await
                .unwrap();

            backend
        }

        #[tokio::test]
        async fn test_get() {
            let backend = backend().await;

            let value = backend
                .get(&c("default"), "key1")
                .await
                .expect("should have no error")
                .expect("should have data");

            assert_eq!(value, Bytes::from("value1"));
        }

        #[tokio::test]
        async fn test_get_container() {
            let backend = backend().await;

            let value = backend
                .get(&c("container1"), "key1")
                .await
                .expect("should have no error")
                .expect("should have data");

            assert_eq!(value, Bytes::from("value1"));
        }

        #[tokio::test]
        async fn test_exists() {
            let backend = backend().await;

            assert!(backend.exists(&c("default"), "key1").await.unwrap());
            assert!(backend.exists(&c("default"), "key2").await.unwrap());
            assert!(!backend.exists(&c("default"), "key3").await.unwrap());
        }

        #[tokio::test]
        async fn test_exists_container() {
            let backend = backend().await;

            assert!(backend.exists(&c("container1"), "key1").await.unwrap());
            assert!(backend.exists(&c("container1"), "key2").await.unwrap());
            assert!(!backend.exists(&c("container1"), "key3").await.unwrap());
        }

        #[tokio::test]
        async fn test_get_stream() {
            let backend = backend().await;

            let mut stream = backend
                .get_stream(&c("default"), "key1")
                .await
                .expect("should have no error")
                .expect("should have data");

            let mut value = BytesMut::new();
            while let Some(chunk) = stream.next().await {
                value.extend_from_slice(
                    &chunk.expect("chunk should be readable"),
                );
            }

            assert_eq!(value, Bytes::from("value1"));
        }

        #[tokio::test]
        async fn test_get_stream_container() {
            let backend = backend().await;

            let mut stream = backend
                .get_stream(&c("container1"), "key1")
                .await
                .expect("should have no error")
                .expect("should have data");

            let mut value = BytesMut::new();
            while let Some(chunk) = stream.next().await {
                value.extend_from_slice(
                    &chunk.expect("chunk should be readable"),
                );
            }

            assert_eq!(value, Bytes::from("value1"));
        }

        #[tokio::test]
        async fn test_list() {
            let backend = backend().await;

            let mut list = backend.list(&c("default")).await.unwrap();
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
                .list(&c("container1"))
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
                .save(&c("default"), "key3", Bytes::from("value3"))
                .await
                .unwrap();

            let mut list = backend.list(&c("default")).await.unwrap();
            list.sort_by_key(|item| item.key.clone());

            assert_eq!(list.len(), 3);
            assert_eq!(list[0].key, "key1");
            assert_eq!(list[0].size, ByteSize::b(6));
            assert_eq!(list[1].key, "key2");
            assert_eq!(list[1].size, ByteSize::b(6));
            assert_eq!(list[2].key, "key3");
            assert_eq!(list[2].size, ByteSize::b(6));
        }

        #[tokio::test]
        async fn test_save_container() {
            let backend = backend().await;

            backend
                .save(&c("container2"), "key1", Bytes::from("value3"))
                .await
                .unwrap();

            let mut list = backend.list(&c("container2")).await.unwrap();
            list.sort_by_key(|item| item.key.clone());

            assert_eq!(list.len(), 1);
            assert_eq!(list[0].key, "key1");
            assert_eq!(list[0].size, ByteSize::b(6));
        }

        #[tokio::test]
        async fn test_save_stream() {
            let backend = backend().await;

            let stream = futures::stream::once(std::future::ready(
                Bytes::from("value3"),
            ));

            backend
                .save_stream(&c("default"), "key3", Box::pin(stream))
                .await
                .unwrap();

            let mut list = backend.list(&c("default")).await.unwrap();
            list.sort_by_key(|item| item.key.clone());

            assert_eq!(list.len(), 3);
            assert_eq!(list[0].key, "key1");
            assert_eq!(list[0].size, ByteSize::b(6));
            assert_eq!(list[1].key, "key2");
            assert_eq!(list[1].size, ByteSize::b(6));
            assert_eq!(list[2].key, "key3");
            assert_eq!(list[2].size, ByteSize::b(6));
        }

        #[tokio::test]
        async fn test_save_stream_container() {
            let backend = backend().await;

            let stream = futures::stream::once(std::future::ready(
                Bytes::from("value3"),
            ));

            backend
                .save_stream(&c("container2"), "key1", Box::pin(stream))
                .await
                .expect("should have no error");

            let mut list = backend
                .list(&c("container2"))
                .await
                .expect("should have no error");
            list.sort_by_key(|item| item.key.clone());

            assert_eq!(list.len(), 1);
            assert_eq!(list[0].key, "key1");
            assert_eq!(list[0].size, ByteSize::b(6));
        }

        #[tokio::test]
        async fn test_delete() {
            let backend = backend().await;

            backend.delete(&c("default"), "key1").await.unwrap();
            backend.delete(&c("default"), "key2").await.unwrap();

            let list_default = backend.list(&c("default")).await.unwrap();
            assert_eq!(list_default.len(), 0);
        }

        #[tokio::test]
        async fn test_delete_container() {
            let backend = backend().await;

            backend.delete(&c("container1"), "key1").await.unwrap();
            backend.delete(&c("container1"), "key2").await.unwrap();

            let list = backend.list(&c("container1")).await.unwrap();
            assert_eq!(list.len(), 0);
        }

        #[tokio::test]
        async fn test_sibling_containers_are_isolated() {
            let backend = backend().await;

            backend
                .save(&c("ns/ci"), "a", Bytes::from("1"))
                .await
                .unwrap();
            backend
                .save(&c("ns/ci2"), "b", Bytes::from("2"))
                .await
                .unwrap();

            let list = backend.list(&c("ns/ci")).await.unwrap();
            assert_eq!(list.len(), 1);
            assert!(list[0].key.ends_with('a'));
            assert!(backend.get(&c("ns/ci"), "b").await.unwrap().is_none());
        }
    };
}
