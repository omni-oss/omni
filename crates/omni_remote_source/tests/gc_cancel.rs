//! Cancelling a prune mid-removal must not release the remote lock or the lease
//! until the removal has finished: otherwise a peer could publish and lease a
//! replacement that the abandoned removal then deletes.

use std::{
    borrow::Cow,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use omni_remote_source::{
    driver::{
        Connection, DriverContext, DriverRegistry, SourceDriver, SourceSpec,
    },
    error::Error,
    lease::SourceLease,
    manager::{
        RemoteSourceManager, StorePruneFilter, config::RemoteSourceConfig,
    },
};
use omni_utils::lock::LockGuard;
use system_traits::{impls::RealSys, *};
use tokio::{sync::Notify, time::timeout};

const WAIT: Duration = Duration::from_secs(15);

/// A real filesystem whose recursive directory removal waits to be released.
#[derive(Clone, Default)]
struct PausingSys {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

impl BaseFsOpen for PausingSys {
    type File = <RealSys as BaseFsOpen>::File;
    fn base_fs_open(
        &self,
        path: &Path,
        options: &OpenOptions,
    ) -> io::Result<Self::File> {
        RealSys.fs_open(path, options)
    }
}

impl BaseFsRename for PausingSys {
    fn base_fs_rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        RealSys.fs_rename(from, to)
    }
}

impl BaseFsRemoveFile for PausingSys {
    fn base_fs_remove_file(&self, path: &Path) -> io::Result<()> {
        RealSys.fs_remove_file(path)
    }
}

#[async_trait::async_trait]
impl BaseFsReadAsync for PausingSys {
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        RealSys.fs_read_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsMetadataAsync for PausingSys {
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
impl BaseFsWriteAsync for PausingSys {
    async fn base_fs_write_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        RealSys.fs_write_async(path, data).await
    }
}

#[async_trait::async_trait]
impl BaseFsCreateDirAsync for PausingSys {
    async fn base_fs_create_dir_async(
        &self,
        path: &Path,
        options: &CreateDirOptions,
    ) -> io::Result<()> {
        RealSys.fs_create_dir_async(path, options).await
    }
}

#[async_trait::async_trait]
impl BaseFsReadDirAsync for PausingSys {
    async fn base_fs_read_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        RealSys.fs_read_dir_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveDirAllAsync for PausingSys {
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        // Pause only the checkout's own removal, not the empty-remote cleanup.
        if path.ends_with("trees/fake/remote/p1") {
            self.entered.notify_one();
            self.release.notified().await;
        }
        RealSys.fs_remove_dir_all_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveFileAsync for PausingSys {
    async fn base_fs_remove_file_async(&self, path: &Path) -> io::Result<()> {
        RealSys.fs_remove_file_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRenameAsync for PausingSys {
    async fn base_fs_rename_async(
        &self,
        from: &Path,
        to: &Path,
    ) -> io::Result<()> {
        RealSys.fs_rename_async(from, to).await
    }
}

struct Driver;

#[async_trait::async_trait]
impl SourceDriver<PausingSys> for Driver {
    fn kind(&self) -> &'static str {
        "fake"
    }
    fn slug(&self, _spec: &SourceSpec) -> String {
        "remote".into()
    }
    async fn resolve(
        &self,
        ctx: &DriverContext<'_, PausingSys>,
        spec: &SourceSpec,
        _locked: Option<&str>,
    ) -> Result<String, Error> {
        std::fs::create_dir_all(&ctx.raw_dir)?;
        Ok(spec.selector.clone())
    }
    async fn export(
        &self,
        _ctx: &DriverContext<'_, PausingSys>,
        _spec: &SourceSpec,
        pin: &str,
        dest: &Path,
    ) -> Result<(), Error> {
        std::fs::write(dest.join("file.txt"), pin)?;
        Ok(())
    }
}

fn spec() -> SourceSpec {
    SourceSpec {
        locator: "locator".into(),
        selector: "p1".into(),
        connection: Connection::new("locator"),
    }
}

#[tokio::test]
async fn a_cancelled_prune_keeps_its_guards_until_the_removal_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let sys = PausingSys::default();
    let mut drivers = DriverRegistry::default();
    drivers.register(Arc::new(Driver));
    let config = RemoteSourceConfig::builder()
        .lockfile_path(dir.path().join("lock.json"))
        .store_root_path(&store)
        .build();
    let manager = Arc::new(
        RemoteSourceManager::with_drivers(config, sys.clone(), drivers)
            .await
            .unwrap(),
    );
    manager.materialize_with("fake", &spec()).await.unwrap();

    let worker = Arc::clone(&manager);
    let task = tokio::spawn(async move {
        let all = StorePruneFilter {
            all: true,
            ..Default::default()
        };
        worker.prune_store_report(&all, false).await
    });
    timeout(WAIT, sys.entered.notified()).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());

    // The caller is gone, but the removal is still running and must still hold
    // both the lease and the remote lock.
    assert!(
        SourceLease::try_acquire_exclusive(&store, "fake", "remote", "p1")
            .await
            .unwrap()
            .is_none(),
        "the lease was released while the removal was still running"
    );
    let remote_lock = store.join("locks/fake/remote.lock");
    assert!(
        LockGuard::try_acquire_exclusive(&remote_lock)
            .await
            .unwrap()
            .is_none(),
        "the remote lock was released while the removal was still running"
    );

    sys.release.notify_one();
    let guard = timeout(WAIT, LockGuard::acquire_exclusive(&remote_lock))
        .await
        .unwrap()
        .unwrap();
    // Both guards drop together; take the lease once the removal is done.
    drop(guard);
    assert!(
        timeout(
            WAIT,
            SourceLease::try_acquire_exclusive(&store, "fake", "remote", "p1")
        )
        .await
        .unwrap()
        .unwrap()
        .is_some()
    );
    assert!(!store.join("trees/fake/remote/p1").exists());
}
