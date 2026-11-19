#![cfg(unix)]

use bytes::Bytes;
use omni_remote_cache_storage_core::{
    ByteRange, Container, RemoteCacheStorageBackend,
};
use omni_remote_cache_storage_local::LocalDiskCacheBackend;

/// A container `ns/ci` holding one real object, plus a symlink inside it that
/// points at a directory outside the backend root holding a secret file.
async fn fixture()
-> (tempfile::TempDir, LocalDiskCacheBackend, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret"), "top-secret").unwrap();

    let backend = LocalDiskCacheBackend::new(&root);
    let ci = Container::parse("ns/ci").unwrap();
    backend
        .save_if_absent(&ci, "a", Bytes::from("kept"))
        .await
        .unwrap();
    std::os::unix::fs::symlink(&outside, root.join("ns/ci/link")).unwrap();
    (tmp, backend, outside)
}

#[tokio::test]
async fn symlinked_directories_are_never_listed_counted_or_followed() {
    let (_tmp, backend, _outside) = fixture().await;
    let prefix = Container::parse("ns/ci").unwrap().prefix();

    let page = backend.list_prefix(&prefix, None, 100).await.unwrap();
    let keys: Vec<_> = page.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["a"], "the symlinked directory must not be walked");
    assert!(page.next_after.is_none());

    assert_eq!(
        backend
            .list_children(&Container::parse("ns").unwrap().prefix())
            .await
            .unwrap(),
        ["ci"]
    );
    assert!(backend.list_children(&prefix).await.unwrap().is_empty());

    let secret = Container::parse("ns/ci/link").unwrap();
    assert_eq!(
        backend
            .get_range(&secret, "secret", ByteRange::Suffix(6))
            .await
            .unwrap(),
        None,
        "get_range must not read through a symlinked container"
    );
    assert!(
        backend
            .list_prefix(&secret.prefix(), None, 100)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn delete_prefix_never_removes_outside_the_root_through_a_symlink() {
    let (_tmp, backend, outside) = fixture().await;

    assert!(
        backend
            .delete_prefix(&Container::parse("ns/ci/link").unwrap().prefix())
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("secret")).unwrap(),
        "top-secret",
        "the outside file must survive"
    );

    assert_eq!(
        backend
            .delete_prefix(&Container::parse("ns/ci").unwrap().prefix())
            .await
            .unwrap(),
        1,
        "the symlink is not a regular file and is not counted"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("secret")).unwrap(),
        "top-secret",
        "the outside file must survive delete_prefix"
    );
    assert!(std::fs::symlink_metadata(outside.join("secret")).is_ok());
}

#[tokio::test]
async fn an_empty_leftover_directory_is_not_a_child() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let backend = LocalDiskCacheBackend::new(&root);
    backend
        .save_if_absent(
            &Container::parse("ns/full").unwrap(),
            "a",
            Bytes::from("x"),
        )
        .await
        .unwrap();
    std::fs::create_dir_all(root.join("ns/empty/deeper")).unwrap();
    let prefix = Container::parse("ns").unwrap().prefix();
    assert_eq!(backend.list_children(&prefix).await.unwrap(), ["full"]);
}

#[tokio::test]
async fn a_non_utf8_name_is_not_listed_but_does_not_block_a_purge() {
    use std::os::unix::ffi::OsStrExt;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let backend = LocalDiskCacheBackend::new(&root);
    let ci = Container::parse("ns/ci").unwrap();
    backend
        .save_if_absent(&ci, "a", Bytes::from("x"))
        .await
        .unwrap();
    let odd = root
        .join("ns/ci")
        .join(std::ffi::OsStr::from_bytes(b"bad\xff"));
    // Some file systems (APFS on macOS) refuse names that are not valid UTF-8,
    // so there is nothing to list or purge there.
    if std::fs::write(&odd, "junk").is_err() {
        return;
    }

    let keys: Vec<String> = backend
        .list(&ci)
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.key)
        .collect();
    assert_eq!(keys, ["a"]);
    assert_eq!(backend.delete_prefix(&ci.prefix()).await.unwrap(), 2);
    assert!(!odd.exists());
}
