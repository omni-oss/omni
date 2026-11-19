//! Pause a real manifest read so prune runs at the exact vulnerable boundary.

use std::{
    borrow::Cow,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use super::*;
use system_traits::*;
use tokio::{sync::Notify, time::timeout};

const WAIT: Duration = Duration::from_secs(20);

#[derive(Clone, Default)]
struct ReadingSys {
    entered: Arc<Notify>,
    release: Arc<Notify>,
    fail_read: bool,
}

impl BaseFsOpen for ReadingSys {
    type File = <RealSys as BaseFsOpen>::File;
    fn base_fs_open(
        &self,
        path: &Path,
        options: &OpenOptions,
    ) -> io::Result<Self::File> {
        RealSys.fs_open(path, options)
    }
}

impl BaseFsRename for ReadingSys {
    fn base_fs_rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        RealSys.fs_rename(from, to)
    }
}

impl BaseFsRemoveFile for ReadingSys {
    fn base_fs_remove_file(&self, path: &Path) -> io::Result<()> {
        RealSys.fs_remove_file(path)
    }
}

#[async_trait::async_trait]
impl BaseFsReadAsync for ReadingSys {
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        // Pause on a local child of a git root, not the first manifest. Its
        // parent's lease must survive returning from the parent's classify.
        if path.ends_with("inner/projection.omni.yaml") {
            self.entered.notify_one();
            timeout(WAIT, self.release.notified())
                .await
                .expect("manifest read was not released");
            if self.fail_read {
                return Err(io::Error::other("injected manifest read failure"));
            }
        }
        RealSys.fs_read_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsMetadataAsync for ReadingSys {
    type Metadata = <RealSys as BaseFsMetadataAsync>::Metadata;
    async fn base_fs_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        RealSys.fs_metadata_async(path).await
    }
    async fn base_fs_symlink_metadata_async(
        &self,
        path: &Path,
    ) -> io::Result<Self::Metadata> {
        RealSys.fs_symlink_metadata_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsWriteAsync for ReadingSys {
    async fn base_fs_write_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        RealSys.fs_write_async(path, data).await
    }
}

#[async_trait::async_trait]
impl BaseFsCreateDirAsync for ReadingSys {
    async fn base_fs_create_dir_async(
        &self,
        path: &Path,
        options: &CreateDirOptions,
    ) -> io::Result<()> {
        RealSys.fs_create_dir_async(path, options).await
    }
}

#[async_trait::async_trait]
impl BaseFsReadDirAsync for ReadingSys {
    async fn base_fs_read_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        RealSys.fs_read_dir_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveDirAllAsync for ReadingSys {
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        RealSys.fs_remove_dir_all_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveFileAsync for ReadingSys {
    async fn base_fs_remove_file_async(&self, path: &Path) -> io::Result<()> {
        RealSys.fs_remove_file_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRenameAsync for ReadingSys {
    async fn base_fs_rename_async(
        &self,
        from: &Path,
        to: &Path,
    ) -> io::Result<()> {
        RealSys.fs_rename_async(from, to).await
    }
}

#[derive(Clone, Copy)]
enum Finish {
    Success,
    ReadError,
    Cancel,
}

async fn projection_read_is_protected(finish: Finish) {
    assert!(git_available(), "this regression requires git on PATH");
    let (_repo, url, _) = build_repo(&[
        (
            "projection.omni.yaml",
            "name: \"@vendor/bundle\"\nsources:\n  - source: local\n    path: ./inner\n    id: child\n",
        ),
        (
            "inner/projection.omni.yaml",
            "name: \"@vendor/child\"\nroutes:\n  - strategy: mirror\n",
        ),
    ]);
    let (_unrelated, unrelated_url, _) =
        build_repo(&[("unrelated.txt", "not being read")]);
    let (ws, sources_root) = workspace();
    let sys = ReadingSys {
        fail_read: matches!(finish, Finish::ReadError),
        ..Default::default()
    };
    let manager = RemoteSourceManager::new(
        RemoteSourceConfig::builder()
            .lockfile_path(sources_root.join("lock.json"))
            .store_root_path(sources_root.join("store"))
            .build(),
        sys.clone(),
    )
    .await
    .unwrap();
    // Use a separate manager for prune, as a cooperating peer would.
    let peer = super::manager(&sources_root).await;
    let unrelated = peer
        .materialize(&RemoteSource::Git {
            uri: unrelated_url,
            rev: "main".into(),
        })
        .await
        .unwrap();
    let source = serde_json::from_value(serde_json::json!({
        "source": "git", "uri": url, "rev": "main", "id": "bundle"
    }))
    .unwrap();
    let contributor = ProjectionRemoteContributor::new(vec![source], ws.path());
    let mut read = Box::pin(
        contributor.contribute(&manager, &InstallOptions { update: false }),
    );
    // Drive the read until the barrier, without a detached task that could
    // survive a failing assertion.
    tokio::select! {
        result = &mut read => panic!("contribution finished before its read barrier: {result:?}"),
        result = timeout(WAIT, sys.entered.notified()) => result.expect("manifest read never started"),
    }
    let all = StorePruneFilter {
        all: true,
        ..Default::default()
    };
    let report = timeout(WAIT, peer.prune_store_report(&all, false))
        .await
        .expect("prune blocked on a reader")
        .unwrap();
    assert_eq!(report.skipped_in_use.len(), 1, "reader was not leased");
    assert_eq!(report.removed.len(), 1, "unrelated checkout not reclaimed");
    assert!(!unrelated.root.exists());
    assert!(report.failed.is_empty());

    if matches!(finish, Finish::Cancel) {
        drop(read);
    } else {
        sys.release.notify_one();
        let result =
            timeout(WAIT, &mut read).await.expect("read did not finish");
        match finish {
            Finish::Success => assert_eq!(result.unwrap().len(), 1),
            Finish::ReadError => {
                let error = result.expect_err("injected read should fail");
                assert!(
                    format!("{error:?}")
                        .contains("injected manifest read failure"),
                    "{error:?}"
                );
            }
            Finish::Cancel => unreachable!(),
        }
        drop(read);
    }
    let report = timeout(WAIT, peer.prune_store_report(&all, false))
        .await
        .expect("prune blocked after contribution ended")
        .unwrap();
    assert!(report.skipped_in_use.is_empty(), "lease leaked");
    assert!(report.failed.is_empty());
    assert_eq!(report.removed.len(), 1, "finished reader still retained");
}

#[tokio::test]
async fn projection_install_leases_nested_manifest_reads() {
    projection_read_is_protected(Finish::Success).await;
}

#[tokio::test]
async fn projection_install_read_error_releases_leases() {
    projection_read_is_protected(Finish::ReadError).await;
}

#[tokio::test]
async fn cancelling_projection_install_releases_leases() {
    projection_read_is_protected(Finish::Cancel).await;
}
