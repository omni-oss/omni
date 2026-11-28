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
    BaseFsReadAsync, FsRemoveFileAsync, FsWriteAsync, impls::RealSys,
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
async fn an_overlay_miss_reads_the_disk_through_the_view_and_mirrors_it() {
    let setup = tokio::task::spawn_blocking(setup).await.unwrap();
    if !contained_supported(&setup).await {
        return;
    }
    let path = setup.root.join("disk.txt");
    let bytes = setup.reader.base_fs_read_async(&path).await.unwrap();
    assert_eq!(&*bytes, b"on disk");
    // The fetched content is mirrored into the overlay (as for a plain read):
    // after the file changes on disk the overlay still answers.
    fs::write(&path, "changed on disk").unwrap();
    let again = setup.reader.base_fs_read_async(&path).await.unwrap();
    assert_eq!(&*again, b"on disk");
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
