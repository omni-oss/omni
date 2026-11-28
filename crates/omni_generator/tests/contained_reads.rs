//! Contained reads composed with the generator's transactional overlay: the
//! overlay (uncommitted writes, deletions, dry-run state) answers first, a miss
//! reads the disk through the input view, and a denied path never reaches the
//! overlay.

use std::{fs, io, path::Path};

use omni_capabilities::{CapabilityRules, PathRoots, Root};
use omni_capability_sys::{EvaluatingAuthorizer, PolicyEnforcingSys};
use omni_contained_read::{ContainedReader, SharedAuthorizer, input_limits};
use omni_containment_configurations::FsReadLimits;
use omni_generator::TransactionSys;
use system_traits::{
    BaseFsReadAsync, FsRemoveFileAsync, FsRenameAsync, FsWriteAsync,
    impls::RealSys,
};

type Shared = SharedAuthorizer<EvaluatingAuthorizer<(), Root>>;

const POLICY: &str = r#"[
    {"access":"allow","domain":"fs.read","patterns":["@workspace/**"]},
    {"access":"deny","domain":"fs.read","patterns":["@workspace/secret/**"]},
    {"access":"allow","domain":"fs.write","patterns":["@workspace/**"]}
]"#;

struct Setup {
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
    tx: TransactionSys<RealSys>,
    reader: ContainedReader<
        PolicyEnforcingSys<TransactionSys<RealSys>, Shared>,
        TransactionSys<RealSys>,
    >,
}

fn setup() -> Setup {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap().join("ws");
    fs::create_dir_all(root.join("secret")).unwrap();
    fs::write(root.join("disk.txt"), "on disk").unwrap();
    fs::write(root.join("secret").join("key"), "SECRET").unwrap();

    let rules: CapabilityRules = serde_json::from_str(POLICY).unwrap();
    let shared = SharedAuthorizer::new(EvaluatingAuthorizer::new(
        rules,
        PathRoots::new().with(Root::Workspace, &root),
        (),
    ));
    let tx = TransactionSys::new(RealSys);
    let broker = PolicyEnforcingSys::new(tx.clone(), shared.clone());
    let reader = ContainedReader::new(
        broker,
        tx.clone(),
        shared,
        Some(root.clone()),
        input_limits(&FsReadLimits::DEFAULT),
    );
    Setup {
        _tmp: tmp,
        root,
        tx,
        reader,
    }
}

async fn contained_supported(setup: &Setup) -> bool {
    match setup
        .reader
        .base_fs_read_async(&setup.root.join("disk.txt"))
        .await
    {
        Ok(_) => true,
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            let by_design = cfg!(target_os = "macos")
                || (cfg!(target_os = "linux")
                    && (!cfg!(target_arch = "x86_64")
                        || std::env::var_os(
                            "OMNI_INPUT_VIEW_ALLOW_NO_OPENAT2",
                        )
                        .is_some()));
            assert!(by_design, "this target must not answer Unsupported: {e}");
            let _ = omni_test_capabilities::gate(
                "openat2",
                false,
                "containment is unsupported on this target",
            );
            false
        }
        Err(e) => panic!("unexpected probe result: {e}"),
    }
}

/// A host action's read mirrors the bytes it fetched into the overlay under the
/// path it read. Those bytes are not proof of anything: a symlink can point
/// outside the workspace, so a contained read must re-read them through the
/// view instead of serving the mirror.
#[tokio::test(flavor = "multi_thread")]
async fn a_host_read_of_a_symlink_does_not_hand_outside_bytes_to_a_script() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    if !contained_supported(&setup).await
        || !omni_test_capabilities::require(
            omni_test_capabilities::Capability::Symlink,
            &setup.root,
        )
    {
        return;
    }
    let outside = setup.root.parent().unwrap().join("outside-secret.txt");
    fs::write(&outside, "OUTSIDE").unwrap();
    let link = setup.root.join("link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, &link).unwrap();

    // The generator reads its template through the transaction (this is what a
    // native `add` action does): the bytes land in the overlay mirror.
    let mirrored = setup.tx.base_fs_read_async(&link).await.unwrap();
    assert_eq!(&*mirrored, b"OUTSIDE", "the host read follows the link");
    assert_eq!(
        &*setup.tx.base_fs_read_async(&link).await.unwrap(),
        b"OUTSIDE"
    );

    // A script read of the same path must be refused, not served from the
    // mirror.
    let error = setup.reader.base_fs_read_async(&link).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    let text = error.to_string();
    assert!(
        text.starts_with("input_view.symlink")
            || text.starts_with("input_view.reparse_point"),
        "{text}"
    );
    assert!(!text.contains("OUTSIDE"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_read_of_an_oversized_file_is_still_limited_for_a_script() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    if !contained_supported(&setup).await {
        return;
    }
    // One byte above the default `max_file_bytes` (8 MiB).
    let big = setup.root.join("big.bin");
    fs::write(&big, vec![b'a'; 8 * 1024 * 1024 + 1]).unwrap();
    // A host action reads it: the mirror now holds the whole file.
    let mirrored = setup.tx.base_fs_read_async(&big).await.unwrap();
    assert_eq!(mirrored.len(), 8 * 1024 * 1024 + 1);

    // The script read goes through the view, so the limit applies.
    let error = setup.reader.base_fs_read_async(&big).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{error}");
    assert!(
        error.to_string().starts_with("input_view.too_large"),
        "{error}"
    );

    // A buffered deletion still shadows, even with a mirror in place.
    setup.tx.fs_remove_file_async(&big).await.unwrap();
    let error = setup.reader.base_fs_read_async(&big).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

/// A committed transaction drops its buffered view, so a path it once wrote is
/// no longer a pending write. If the file is later replaced by a symlink, a
/// host read mirrors the target's bytes; a contained read must still check it.
#[tokio::test(flavor = "multi_thread")]
async fn a_committed_transaction_does_not_keep_a_path_a_pending_write() {
    for use_transaction_commit in [false, true] {
        let setup = tokio::task::spawn_blocking(setup).await.unwrap();
        if !contained_supported(&setup).await
            || !omni_test_capabilities::require(
                omni_test_capabilities::Capability::Symlink,
                &setup.root,
            )
        {
            return;
        }
        let outside = setup.root.parent().unwrap().join("outside-commit.txt");
        fs::write(&outside, "OUTSIDE").unwrap();
        let path = setup.root.join("reused.txt");

        setup.tx.fs_write_async(&path, b"written").await.unwrap();
        if use_transaction_commit {
            setup.tx.begin_transaction().await;
            setup.tx.fs_write_async(&path, b"written").await.unwrap();
            setup.tx.commit_transaction().await.unwrap();
        } else {
            setup.tx.commit().await.unwrap();
        }
        assert_eq!(fs::read(&path).unwrap(), b"written", "flushed to disk");

        // The committed path is no longer buffered; replace it with a link.
        fs::remove_file(&path).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&outside, &path).unwrap();

        let mirrored = setup.tx.base_fs_read_async(&path).await.unwrap();
        assert_eq!(&*mirrored, b"OUTSIDE");

        let error = setup.reader.base_fs_read_async(&path).await.unwrap_err();
        assert_eq!(
            error.kind(),
            io::ErrorKind::PermissionDenied,
            "commit mode {use_transaction_commit}: {error}"
        );
        assert!(!error.to_string().contains("OUTSIDE"));
        let text = error.to_string();
        assert!(
            text.starts_with("input_view.symlink")
                || text.starts_with("input_view.reparse_point"),
            "{text}"
        );
    }
}

/// A rename is a write route: the destination holds this transaction's own
/// content, so a contained read of it must see the renamed bytes, not the
/// pre-rename disk state.
#[tokio::test(flavor = "multi_thread")]
async fn a_contained_read_sees_a_renamed_file_and_directory() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    if !contained_supported(&setup).await {
        return;
    }
    // Disk-backed source, no pending write.
    let from = setup.root.join("disk.txt");
    let to = setup.root.join("renamed.txt");
    setup.tx.fs_rename_async(&from, &to).await.unwrap();
    assert!(from.exists(), "the rename is buffered, not on disk");
    let bytes = setup.reader.base_fs_read_async(&to).await.unwrap();
    assert_eq!(&*bytes, b"on disk");
    let gone = setup.reader.base_fs_read_async(&from).await.unwrap_err();
    assert_eq!(gone.kind(), io::ErrorKind::NotFound);

    // A disk-backed directory rename materializes each descendant.
    let dir = setup.root.join("tree");
    fs::create_dir_all(dir.join("nested")).unwrap();
    fs::write(dir.join("nested").join("leaf"), "leaf bytes").unwrap();
    let moved = setup.root.join("tree-moved");
    setup.tx.fs_rename_async(&dir, &moved).await.unwrap();
    let leaf = setup
        .reader
        .base_fs_read_async(&moved.join("nested").join("leaf"))
        .await
        .unwrap();
    assert_eq!(&*leaf, b"leaf bytes");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_is_read_back_from_the_overlay_not_the_disk() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    let new = setup.root.join("new.txt");
    setup.tx.fs_write_async(&new, b"uncommitted").await.unwrap();
    assert!(!new.exists(), "the write is buffered, not on disk");
    let bytes = setup.reader.base_fs_read_async(&new).await.unwrap();
    assert_eq!(&*bytes, b"uncommitted");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_overwrite_of_a_disk_file_wins_over_the_disk_bytes() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    let path = setup.root.join("disk.txt");
    setup
        .tx
        .fs_write_async(&path, b"overwritten")
        .await
        .unwrap();
    let bytes = setup.reader.base_fs_read_async(&path).await.unwrap();
    assert_eq!(&*bytes, b"overwritten");
    assert_eq!(fs::read(&path).unwrap(), b"on disk");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_buffered_deletion_reads_as_not_found() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    let path = setup.root.join("disk.txt");
    setup.tx.fs_remove_file_async(&path).await.unwrap();
    assert!(path.exists(), "the deletion is buffered, not on disk");
    let error = setup.reader.base_fs_read_async(&path).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_contained_read_observes_the_disk_and_mirrors_it_for_host_reads() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    if !contained_supported(&setup).await {
        return;
    }
    let path = setup.root.join("disk.txt");
    let bytes = setup.reader.base_fs_read_async(&path).await.unwrap();
    assert_eq!(&*bytes, b"on disk");
    // A check-free mirror is never served to a script: each contained read
    // observes the file as it is now, so a change on disk is visible.
    fs::write(&path, "changed on disk").unwrap();
    let again = setup.reader.base_fs_read_async(&path).await.unwrap();
    assert_eq!(&*again, b"changed on disk");
    // The bytes a contained read fetched are still mirrored for host readers.
    let host = setup.tx.base_fs_read_async(&path).await.unwrap();
    assert_eq!(&*host, b"changed on disk");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_path_with_an_overlay_entry_is_refused() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    let path = setup.root.join("secret").join("key");
    setup
        .tx
        .fs_write_async(&path, b"from the overlay")
        .await
        .unwrap();
    let error = setup.reader.base_fs_read_async(&path).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(
        error.to_string().starts_with("input_view.denied"),
        "{error}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relative_path_is_anchored_by_the_overlay_cwd() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    if !contained_supported(&setup).await {
        return;
    }
    // `set-current-dir` is buffered in the overlay and never touches the
    // process cwd.
    use system_traits::EnvSetCurrentDirAsync;
    setup
        .tx
        .env_set_current_dir_async(&setup.root)
        .await
        .unwrap();
    let bytes = setup
        .reader
        .base_fs_read_async(Path::new("disk.txt"))
        .await
        .unwrap();
    assert_eq!(&*bytes, b"on disk");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_symlink_read_is_refused_and_not_mirrored() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    if !contained_supported(&setup).await
        || !omni_test_capabilities::require(
            omni_test_capabilities::Capability::Symlink,
            &setup.root,
        )
    {
        return;
    }
    let link = setup.root.join("link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(setup.root.join("disk.txt"), &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(setup.root.join("disk.txt"), &link)
        .unwrap();
    let error = setup.reader.base_fs_read_async(&link).await.unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{error}");
    let text = error.to_string();
    assert!(
        text.starts_with("input_view.symlink")
            || text.starts_with("input_view.reparse_point"),
        "{text}"
    );
    // A refusal is not cached: nothing is mirrored for the link.
    let again = setup.reader.base_fs_read_async(&link).await.unwrap_err();
    assert_eq!(again.kind(), io::ErrorKind::PermissionDenied);
}
