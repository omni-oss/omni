//! Workspace registrations written with a reference batch (RFC 0024 F08.7).

use std::path::{Path, PathBuf};

use omni_remote_source::{
    RemoteSource, RemoteSourceRef,
    manager::{RemoteSourceManager, config::RemoteSourceConfig},
    refs::{RefSetUpdate, RefUpdate},
    registration::{
        REGISTRATIONS_SEGMENT, RegistrationData, strip_verbatim_prefix,
    },
};
use system_traits::impls::RealSys;

fn reference(pin: &str) -> RemoteSourceRef {
    RemoteSourceRef {
        source: RemoteSource::Git {
            uri: "https://example.com/repo.git".parse().unwrap(),
            rev: "main".to_string(),
        },
        pin: pin.to_string(),
    }
}

async fn manager_at(
    lockfile: PathBuf,
    store: &Path,
) -> RemoteSourceManager<RealSys> {
    RemoteSourceManager::new(
        RemoteSourceConfig::builder()
            .lockfile_path(lockfile)
            .store_root_path(store)
            .build(),
        RealSys,
    )
    .await
    .unwrap()
}

/// A store that has published something: it has a layout marker.
fn store_with_marker(root: &Path, marker: &str) -> PathBuf {
    let store = root.join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join("VERSION"), marker).unwrap();
    store
}

async fn record(manager: &RemoteSourceManager<RealSys>, id: &str, pin: &str) {
    manager
        .update_ref_sets(&[RefSetUpdate {
            id,
            mode: RefUpdate::ReplaceComplete,
            refs: &[reference(pin)],
        }])
        .await
        .unwrap();
}

fn registrations(store: &Path) -> Vec<PathBuf> {
    let dir = store.join(REGISTRATIONS_SEGMENT);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> =
        entries.map(|entry| entry.unwrap().path()).collect();
    files.sort();
    files
}

fn read_registration(file: &Path) -> RegistrationData {
    RegistrationData::from_file_bytes(file, &std::fs::read(file).unwrap())
        .unwrap()
}

fn canonical_lockfile(dir: &Path) -> String {
    strip_verbatim_prefix(
        &std::fs::canonicalize(dir)
            .unwrap()
            .join("lock.json")
            .to_string_lossy(),
    )
}

#[tokio::test]
async fn a_first_install_registers_before_the_lockfile_exists() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "1\n");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;
    assert!(!dir.path().join("lock.json").exists());

    record(&manager, "generator", "p1").await;

    let files = registrations(&store);
    assert_eq!(files.len(), 1);
    assert_eq!(
        read_registration(&files[0]).lockfile_path(),
        canonical_lockfile(dir.path())
    );
    assert!(dir.path().join("refs/generator.json").exists());
}

#[tokio::test]
async fn registering_again_rewrites_the_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "1\n");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;

    record(&manager, "generator", "p1").await;
    record(&manager, "tool", "p2").await;

    assert_eq!(registrations(&store).len(), 1);
}

#[tokio::test]
async fn an_empty_release_still_registers_but_an_empty_batch_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "1\n");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;

    manager.update_ref_sets(&[]).await.unwrap();
    assert!(
        registrations(&store).is_empty(),
        "an empty batch registered"
    );

    manager
        .update_ref_sets(&[RefSetUpdate {
            id: "generator",
            mode: RefUpdate::ReplaceComplete,
            refs: &[],
        }])
        .await
        .unwrap();
    assert_eq!(registrations(&store).len(), 1);
}

#[tokio::test]
async fn a_store_without_a_marker_is_not_given_a_registration_directory() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;

    record(&manager, "generator", "p1").await;

    assert!(
        !store.exists(),
        "registering created a store with nothing in it"
    );
    assert!(dir.path().join("refs/generator.json").exists());
}

#[tokio::test]
async fn a_newer_layout_is_not_written_to_but_references_still_are() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "99\n");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;

    record(&manager, "generator", "p1").await;

    assert!(!store.join(REGISTRATIONS_SEGMENT).exists());
    assert_eq!(
        std::fs::read_to_string(store.join("VERSION")).unwrap(),
        "99\n"
    );
    assert!(dir.path().join("refs/generator.json").exists());
}

#[tokio::test]
async fn an_unreadable_layout_marker_fails_without_writing_references() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "not a number\n");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;

    let result = manager
        .update_ref_sets(&[RefSetUpdate {
            id: "generator",
            mode: RefUpdate::ReplaceComplete,
            refs: &[reference("p1")],
        }])
        .await;

    assert!(result.is_err());
    assert!(!dir.path().join("refs").exists());
}

#[tokio::test]
async fn a_corrupt_registration_is_replaced_by_the_next_write() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "1\n");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;
    record(&manager, "generator", "p1").await;
    let file = registrations(&store).remove(0);
    std::fs::write(&file, b"garbage").unwrap();

    record(&manager, "generator", "p1").await;

    assert_eq!(
        read_registration(&file).lockfile_path(),
        canonical_lockfile(dir.path())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_workspace_reached_through_a_symlink_registers_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "1\n");
    let real = dir.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    record(&manager_at(real.join("lock.json"), &store).await, "a", "p1").await;
    record(&manager_at(link.join("lock.json"), &store).await, "a", "p1").await;

    assert_eq!(registrations(&store).len(), 1);
}

#[cfg(unix)]
#[tokio::test]
async fn an_unwritable_registration_directory_leaves_the_references_alone() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let store = store_with_marker(dir.path(), "1\n");
    let manager = manager_at(dir.path().join("lock.json"), &store).await;
    record(&manager, "generator", "p1").await;
    let refs = dir.path().join("refs/generator.json");
    let old = std::fs::read(&refs).unwrap();
    let registry = store.join(REGISTRATIONS_SEGMENT);
    std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o555))
        .unwrap();
    // Root ignores directory permissions, so the failure cannot be injected.
    let enforced = std::fs::write(registry.join("probe"), b"").is_err();

    let result = manager
        .update_ref_sets(&[RefSetUpdate {
            id: "generator",
            mode: RefUpdate::ReplaceComplete,
            refs: &[reference("p2")],
        }])
        .await;
    std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o755))
        .unwrap();

    if enforced {
        assert!(result.is_err());
        assert_eq!(std::fs::read(&refs).unwrap(), old);
    }
}
