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
                .save_if_absent(&c("default"), "key1", Bytes::from("value1"))
                .await
                .unwrap();
            backend
                .save_if_absent(&c("default"), "key2", Bytes::from("value2"))
                .await
                .unwrap();

            backend
                .save_if_absent(&c("container1"), "key1", Bytes::from("value1"))
                .await
                .unwrap();
            backend
                .save_if_absent(&c("container1"), "key2", Bytes::from("value2"))
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
        async fn test_save_if_absent() {
            let backend = backend().await;

            backend
                .save_if_absent(&c("default"), "key3", Bytes::from("value3"))
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
                .save_if_absent(&c("container2"), "key1", Bytes::from("value3"))
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
                .save_stream_if_absent(&c("default"), "key3", Box::pin(stream.map(Ok)))
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
                .save_stream_if_absent(&c("container2"), "key1", Box::pin(stream.map(Ok)))
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
        async fn test_second_write_to_existing_key_is_rejected_and_keeps_original() {
            let backend = backend().await;

            assert_eq!(
                backend
                    .save_if_absent(&c("default"), "key1", Bytes::from("other"))
                    .await
                    .unwrap(),
                $crate::SaveOutcome::AlreadyExists
            );

            let stream = futures::stream::once(std::future::ready(
                Ok::<_, $crate::error::Error>(Bytes::from("other")),
            ));
            assert_eq!(
                backend
                    .save_stream_if_absent(&c("default"), "key1", Box::pin(stream))
                    .await
                    .unwrap(),
                $crate::SaveOutcome::AlreadyExists
            );

            assert_eq!(
                backend.get(&c("default"), "key1").await.unwrap(),
                Some(Bytes::from("value1"))
            );
        }

        #[tokio::test]
        async fn test_concurrent_new_key_writers_store_exactly_one_winner() {
            let backend = std::sync::Arc::new(backend().await);
            let mut tasks = Vec::new();
            for i in 0..16 {
                let writer = backend.clone();
                let bytes = Bytes::from(format!("writer-{i}"));
                tasks.push(tokio::spawn(async move {
                    let stream = futures::stream::once(std::future::ready(
                        Ok::<_, $crate::error::Error>(bytes.clone()),
                    ));
                    let outcome = writer
                        .save_stream_if_absent(
                            &c("default"),
                            "contended",
                            Box::pin(stream),
                        )
                        .await
                        .unwrap();
                    (bytes, outcome)
                }));
            }

            let mut winners = Vec::new();
            let mut rejected = 0;
            for task in tasks {
                let (bytes, outcome) = task.await.unwrap();
                match outcome {
                    $crate::SaveOutcome::Stored => winners.push(bytes),
                    $crate::SaveOutcome::AlreadyExists => rejected += 1,
                }
            }

            assert_eq!(winners.len(), 1, "exactly one writer must store");
            assert_eq!(rejected, 15, "every other writer must be rejected");
            assert_eq!(
                backend.get(&c("default"), "contended").await.unwrap(),
                Some(winners.remove(0))
            );
        }

        #[tokio::test]
        async fn test_failed_upload_preserves_existing_entry_and_does_not_create_new_entry() {
            let backend = backend().await;
            for key in ["key1", "failed-upload"] {
                let stream = futures::stream::iter([
                    Ok(Bytes::from_static(b"partial")),
                    Err($crate::error::Error::custom(std::io::Error::other("interrupted upload"))),
                ]);
                assert!(backend.save_stream_if_absent(&c("default"), key, Box::pin(stream)).await.is_err());
            }
            assert_eq!(backend.get(&c("default"), "key1").await.unwrap(), Some(Bytes::from("value1")));
            assert!(backend.get(&c("default"), "failed-upload").await.unwrap().is_none());
        }

        #[tokio::test]
        async fn test_readers_keep_complete_entry_during_upload_and_cancellation() {
            let backend = std::sync::Arc::new(backend().await);
            let (started, waiting) = tokio::sync::oneshot::channel();
            let (release, gate) = tokio::sync::oneshot::channel::<Result<Bytes, $crate::error::Error>>();
            let stream = futures::stream::once(std::future::ready(Ok(Bytes::from_static(b"partial"))))
                .chain(futures::stream::once(async move {
                    started.send(()).unwrap();
                    gate.await.unwrap()
                }));
            let writer = backend.clone();
            let upload = tokio::spawn(async move {
                writer.save_stream_if_absent(&c("default"), "key1", Box::pin(stream)).await
            });
            tokio::time::timeout(std::time::Duration::from_secs(10), waiting).await.unwrap().unwrap();
            assert_eq!(backend.get(&c("default"), "key1").await.unwrap(), Some(Bytes::from("value1")));
            let list = backend.list(&c("default")).await.unwrap();
            assert_eq!(list.len(), 2, "staging uploads must not appear in listings");
            upload.abort();
            assert!(upload.await.unwrap_err().is_cancelled());
            drop(release);
            assert_eq!(backend.get(&c("default"), "key1").await.unwrap(), Some(Bytes::from("value1")));
        }

        #[tokio::test]
        async fn test_sibling_containers_are_isolated() {
            let backend = backend().await;

            backend
                .save_if_absent(&c("ns/ci"), "a", Bytes::from("1"))
                .await
                .unwrap();
            backend
                .save_if_absent(&c("ns/ci2"), "b", Bytes::from("2"))
                .await
                .unwrap();

            let list = backend.list(&c("ns/ci")).await.unwrap();
            assert_eq!(list.len(), 1);
            assert!(list[0].key.ends_with('a'));
            assert!(backend.get(&c("ns/ci"), "b").await.unwrap().is_none());
        }

        // ---- range reads, prefix operations, pagination ----

        #[tokio::test]
        async fn test_get_range_returns_the_selected_bytes() {
            use $crate::ByteRange::{Bounded, Suffix};
            let backend = backend().await;
            let c = c("default");
            // key1 holds "value1".
            for (range, want) in [
                (Suffix(3), "ue1"),
                (Suffix(100), "value1"),
                (Suffix(0), ""),
                (Bounded(1, 4), "alu"),
                (Bounded(0, 6), "value1"),
                (Bounded(4, 100), "e1"),
                (Bounded(3, 3), ""),
                (Bounded(4, 2), ""),
                (Bounded(10, 12), ""),
                (Bounded(6, 9), ""),
            ] {
                assert_eq!(
                    backend.get_range(&c, "key1", range).await.unwrap(),
                    Some(Bytes::from(want)),
                    "{range:?}"
                );
            }
            assert_eq!(backend.get_range(&c, "nope", Suffix(3)).await.unwrap(), None);
            assert_eq!(backend.get_range(&c, "nope", Bounded(0, 3)).await.unwrap(), None);
        }

        /// Fills `pfx/a` with five keys and `pfx/a/b` with two, plus siblings
        /// `pfx/a2` and a key one level up, so every prefix test has the same
        /// neighbours to trip over.
        async fn fill_prefixes(backend: &impl RemoteCacheStorageBackend) {
            for k in ["k1", "k2", "k3", "k4", "k5"] {
                backend.save_if_absent(&c("pfx/a"), k, Bytes::from(k)).await.unwrap();
            }
            for k in ["z1", "z2"] {
                backend.save_if_absent(&c("pfx/a/b"), k, Bytes::from(k)).await.unwrap();
            }
            backend.save_if_absent(&c("pfx/a2"), "sib", Bytes::from("sib")).await.unwrap();
            backend.save_if_absent(&c("pfx"), "top", Bytes::from("top")).await.unwrap();
        }

        fn keys(page: &$crate::PrefixPage) -> Vec<String> {
            page.items.iter().map(|i| i.key.clone()).collect()
        }

        #[tokio::test]
        async fn test_list_prefix_is_recursive_sorted_and_relative() {
            let backend = backend().await;
            fill_prefixes(&backend).await;
            let page = backend.list_prefix(&c("pfx/a").prefix(), None, 100).await.unwrap();
            assert_eq!(keys(&page), ["b/z1", "b/z2", "k1", "k2", "k3", "k4", "k5"]);
            assert_eq!(page.next_after, None, "a complete listing has no cursor");
            assert_eq!(page.items[2].size, ByteSize::b(2));
            // `pfx/a` must not match `pfx/a2`, and an empty prefix lists nothing.
            let none = backend.list_prefix(&c("pfx/missing").prefix(), None, 100).await.unwrap();
            assert!(none.items.is_empty() && none.next_after.is_none());
        }

        #[tokio::test]
        async fn test_list_prefix_pages_cover_every_key_exactly_once() {
            let backend = backend().await;
            fill_prefixes(&backend).await;
            let prefix = c("pfx/a").prefix();
            for limit in [1u32, 2, 3, 7, 8] {
                let mut seen = Vec::new();
                let mut after: Option<String> = None;
                let mut pages = 0;
                loop {
                    let page = backend
                        .list_prefix(&prefix, after.as_deref(), limit)
                        .await
                        .unwrap();
                    assert!(page.items.len() <= limit as usize, "limit {limit}");
                    seen.extend(keys(&page));
                    pages += 1;
                    assert!(pages < 20, "the cursor must make progress");
                    match page.next_after {
                        Some(next) => after = Some(next),
                        None => break,
                    }
                }
                assert_eq!(
                    seen,
                    ["b/z1", "b/z2", "k1", "k2", "k3", "k4", "k5"],
                    "limit {limit}"
                );
            }
        }

        #[tokio::test]
        async fn test_list_children_names_only_the_next_segment() {
            let backend = backend().await;
            fill_prefixes(&backend).await;
            assert_eq!(
                backend.list_children(&c("pfx").prefix()).await.unwrap(),
                ["a", "a2"],
                "objects directly under the prefix are not children"
            );
            assert_eq!(
                backend.list_children(&c("pfx/a").prefix()).await.unwrap(),
                ["b"]
            );
            assert!(backend.list_children(&c("pfx/a/b").prefix()).await.unwrap().is_empty());
            assert!(backend.list_children(&c("pfx/none").prefix()).await.unwrap().is_empty());
        }

        #[tokio::test]
        async fn test_delete_prefix_removes_only_that_prefix() {
            let backend = backend().await;
            fill_prefixes(&backend).await;
            // Put a streamed write in the cache layer (a no-op for other backends).
            let stream = futures::stream::iter([Ok(Bytes::from("cached"))]);
            backend
                .save_stream_if_absent(&c("pfx/a"), "streamed", Box::pin(stream))
                .await
                .unwrap();

            assert_eq!(backend.delete_prefix(&c("pfx/a").prefix()).await.unwrap(), 8);

            for (container, key) in [("pfx/a", "k1"), ("pfx/a", "streamed"), ("pfx/a/b", "z1")] {
                assert_eq!(backend.get(&c(container), key).await.unwrap(), None, "{container}/{key}");
                assert!(!backend.exists(&c(container), key).await.unwrap());
            }
            assert_eq!(
                backend.get(&c("pfx/a2"), "sib").await.unwrap(),
                Some(Bytes::from("sib")),
                "a sibling prefix must survive"
            );
            assert_eq!(
                backend.get(&c("pfx"), "top").await.unwrap(),
                Some(Bytes::from("top")),
                "a key above the prefix must survive"
            );
            assert_eq!(backend.delete_prefix(&c("pfx/a").prefix()).await.unwrap(), 0);
            assert_eq!(backend.delete_prefix(&c("pfx/never").prefix()).await.unwrap(), 0);
            // The prefix is reusable afterwards.
            assert_eq!(
                backend.save_if_absent(&c("pfx/a"), "k1", Bytes::from("again")).await.unwrap(),
                $crate::SaveOutcome::Stored
            );
        }

        #[tokio::test]
        async fn test_list_prefix_orders_full_keys_bytewise() {
            let backend = backend().await;
            // '-' < '/' < '0' in bytes, so b-x, b/.., b0 is the order.
            for (ctr, k) in [("ord/b", "y"), ("ord", "b0"), ("ord", "b-x"), ("ord", "a")] {
                backend.save_if_absent(&c(ctr), k, Bytes::from("v")).await.unwrap();
            }
            let prefix = c("ord").prefix();
            let page = backend.list_prefix(&prefix, None, 100).await.unwrap();
            assert_eq!(keys(&page), ["a", "b-x", "b/y", "b0"]);
            // A cursor that is not an existing key resumes after its position.
            for (cursor, want) in [
                ("b", vec!["b-x", "b/y", "b0"]),
                ("b-y", vec!["b/y", "b0"]),
                ("b/", vec!["b/y", "b0"]),
                ("zzz", vec![]),
            ] {
                let page = backend.list_prefix(&prefix, Some(cursor), 100).await.unwrap();
                assert_eq!(keys(&page), want, "cursor {cursor:?}");
                assert_eq!(page.next_after, None);
            }
            // The default `list` agrees with the pages.
            let all: Vec<String> =
                backend.list(&c("ord")).await.unwrap().into_iter().map(|i| i.key).collect();
            assert_eq!(all, ["a", "b-x", "b/y", "b0"]);
        }
    };
}
