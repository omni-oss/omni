//! Real-filesystem reference publication, fault injection and concurrency.

use std::{
    borrow::Cow,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU8, AtomicUsize, Ordering},
    },
    time::Duration,
};

use omni_lockfile::Lockfile;
use omni_remote_source::{
    RemoteSource, RemoteSourceRef,
    driver::{DriverContext, DriverRegistry, SourceDriver, SourceSpec},
    manager::{RemoteSourceManager, config::RemoteSourceConfig},
    refs::{RefSetData, RefUpdate},
};
use omni_utils::lock::LockGuard;
use system_traits::{impls::RealSys, *};
use tokio::{process::Command, sync::Notify, time::timeout};

const WAIT: Duration = Duration::from_secs(15);
const OPEN: u8 = 1;
const WRITE: u8 = 2;
const RENAME: u8 = 3;
const READ: u8 = 4;
const PAUSE_RENAME: u8 = 5;

#[derive(Default)]
struct CountingDriver(AtomicUsize);

#[async_trait::async_trait]
impl SourceDriver<FaultSys> for CountingDriver {
    fn kind(&self) -> &'static str {
        "git"
    }
    fn slug(&self, _spec: &SourceSpec) -> String {
        "test-remote".into()
    }

    async fn resolve(
        &self,
        _ctx: &DriverContext<'_, FaultSys>,
        _spec: &SourceSpec,
        locked: Option<&str>,
    ) -> Result<String, omni_remote_source::error::Error> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(locked.unwrap_or("p1").into())
    }

    async fn export(
        &self,
        _ctx: &DriverContext<'_, FaultSys>,
        _spec: &SourceSpec,
        _pin: &str,
        dest: &Path,
    ) -> Result<(), omni_remote_source::error::Error> {
        std::fs::write(dest.join("file.txt"), b"source")?;
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FaultSys {
    fault: Arc<AtomicU8>,
    entered: Arc<Notify>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

fn injected() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "injected failure")
}

impl BaseFsOpen for FaultSys {
    type File = <RealSys as BaseFsOpen>::File;

    fn base_fs_open(
        &self,
        path: &Path,
        options: &OpenOptions,
    ) -> io::Result<Self::File> {
        assert_ne!(path.extension().and_then(|s| s.to_str()), Some("json"));
        if self.fault.load(Ordering::SeqCst) == OPEN {
            return Err(injected());
        }
        let file = RealSys.fs_open(path, options)?;
        if self.fault.load(Ordering::SeqCst) == WRITE {
            drop(file);
            let mut read_only = OpenOptions::new();
            read_only.read = true;
            return RealSys.fs_open(path, &read_only);
        }
        Ok(file)
    }
}

impl BaseFsRename for FaultSys {
    fn base_fs_rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        match self.fault.load(Ordering::SeqCst) {
            RENAME => return Err(injected()),
            PAUSE_RENAME => {
                self.entered.notify_one();
                let (mutex, ready) = &*self.release;
                let released = ready
                    .wait_timeout_while(
                        mutex.lock().unwrap(),
                        WAIT,
                        |released| !*released,
                    )
                    .unwrap();
                assert!(*released.0, "publisher was not released");
            }
            _ => {}
        }
        RealSys.fs_rename(from, to)
    }
}

impl BaseFsRemoveFile for FaultSys {
    fn base_fs_remove_file(&self, path: &Path) -> io::Result<()> {
        RealSys.fs_remove_file(path)
    }
}

#[async_trait::async_trait]
impl BaseFsReadAsync for FaultSys {
    async fn base_fs_read_async(
        &self,
        path: &Path,
    ) -> io::Result<Cow<'static, [u8]>> {
        if self.fault.load(Ordering::SeqCst) == READ {
            return Err(injected());
        }
        RealSys.fs_read_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsMetadataAsync for FaultSys {
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
impl BaseFsWriteAsync for FaultSys {
    async fn base_fs_write_async(
        &self,
        path: &Path,
        data: &[u8],
    ) -> io::Result<()> {
        RealSys.fs_write_async(path, data).await
    }
}

#[async_trait::async_trait]
impl BaseFsCreateDirAsync for FaultSys {
    async fn base_fs_create_dir_async(
        &self,
        path: &Path,
        options: &CreateDirOptions,
    ) -> io::Result<()> {
        RealSys.fs_create_dir_async(path, options).await
    }
}

#[async_trait::async_trait]
impl BaseFsReadDirAsync for FaultSys {
    async fn base_fs_read_dir_async(
        &self,
        path: &Path,
    ) -> io::Result<Vec<PathBuf>> {
        RealSys.fs_read_dir_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveDirAllAsync for FaultSys {
    async fn base_fs_remove_dir_all_async(
        &self,
        path: &Path,
    ) -> io::Result<()> {
        RealSys.fs_remove_dir_all_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRemoveFileAsync for FaultSys {
    async fn base_fs_remove_file_async(&self, path: &Path) -> io::Result<()> {
        RealSys.fs_remove_file_async(path).await
    }
}

#[async_trait::async_trait]
impl BaseFsRenameAsync for FaultSys {
    async fn base_fs_rename_async(
        &self,
        from: &Path,
        to: &Path,
    ) -> io::Result<()> {
        RealSys.fs_rename_async(from, to).await
    }
}

fn config(root: &Path) -> RemoteSourceConfig {
    RemoteSourceConfig::builder()
        .lockfile_path(root.join("lock.json"))
        .store_root_path(root.join("store"))
        .build()
}

fn reference(selector: &str, pin: &str) -> RemoteSourceRef {
    RemoteSourceRef {
        source: RemoteSource::Git {
            uri: "https://example.com/repo.git".parse().unwrap(),
            rev: selector.to_string(),
        },
        pin: pin.to_string(),
    }
}

fn read_refs(root: &Path, id: &str) -> Vec<(String, String)> {
    let data: RefSetData = omni_file_data_serde::from_slice(
        &std::fs::read(root.join(format!("refs/{id}.json"))).unwrap(),
        omni_file_data_serde::Format::Json,
    )
    .unwrap();
    let RefSetData::V1_0_0(data) = data;
    data.sources
        .into_values()
        .flat_map(|locators| {
            locators.into_values().flat_map(|selectors| {
                selectors
                    .into_iter()
                    .map(|(selector, value)| (selector, value.pin))
            })
        })
        .collect()
}

fn assert_no_staging(root: &Path) {
    for entry in std::fs::read_dir(root).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".omni-atomic-")
        );
    }
}

#[tokio::test]
async fn complete_and_additive_updates_are_explicit_and_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let manager = RemoteSourceManager::new(config(dir.path()), RealSys)
        .await
        .unwrap();
    manager
        .record_refs("tool", &[reference("tool", "p0")])
        .await
        .unwrap();
    let tool = std::fs::read(dir.path().join("refs/tool.json")).unwrap();
    manager
        .update_refs(
            "plugin",
            RefUpdate::MergeAdditive,
            &[reference("a", "p1")],
        )
        .await
        .unwrap();
    manager
        .update_refs(
            "plugin",
            RefUpdate::MergeAdditive,
            &[reference("b", "p2"), reference("a", "p3")],
        )
        .await
        .unwrap();
    assert_eq!(
        read_refs(dir.path(), "plugin"),
        [("a".into(), "p3".into()), ("b".into(), "p2".into())]
    );
    manager
        .update_refs(
            "plugin",
            RefUpdate::ReplaceComplete,
            &[reference("c", "p4")],
        )
        .await
        .unwrap();
    assert_eq!(read_refs(dir.path(), "plugin"), [("c".into(), "p4".into())]);
    manager
        .update_refs("plugin", RefUpdate::ReplaceComplete, &[])
        .await
        .unwrap();
    assert!(read_refs(dir.path(), "plugin").is_empty());
    assert_eq!(
        std::fs::read(dir.path().join("refs/tool.json")).unwrap(),
        tool
    );
    assert_no_staging(&dir.path().join("refs"));
}

#[tokio::test]
async fn failed_updates_leave_previous_refs_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let sys = FaultSys::default();
    let manager = RemoteSourceManager::new(config(dir.path()), sys.clone())
        .await
        .unwrap();
    manager
        .record_refs("plugin", &[reference("a", "p1")])
        .await
        .unwrap();
    let path = dir.path().join("refs/plugin.json");
    let old = std::fs::read(&path).unwrap();
    for mode in [RefUpdate::ReplaceComplete, RefUpdate::MergeAdditive] {
        for fault in [OPEN, WRITE, RENAME, READ] {
            sys.fault.store(fault, Ordering::SeqCst);
            assert!(
                manager
                    .update_refs("plugin", mode, &[reference("b", "p2")])
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), old);
            assert_no_staging(path.parent().unwrap());
        }
    }
}

#[tokio::test]
async fn invalid_and_corrupt_sets_are_never_silently_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let manager = RemoteSourceManager::new(config(dir.path()), RealSys)
        .await
        .unwrap();
    for mode in [RefUpdate::ReplaceComplete, RefUpdate::MergeAdditive] {
        for id in ["", ".", "../escape", "a/b", "/absolute"] {
            assert!(manager.update_refs(id, mode, &[]).await.is_err());
        }
        assert!(
            manager
                .update_refs("plugin", mode, &[reference("a", "../bad")])
                .await
                .is_err()
        );
    }
    assert!(!dir.path().join("refs").exists());
    std::fs::create_dir(dir.path().join("refs")).unwrap();
    let path = dir.path().join("refs/plugin.json");
    for bytes in [b"bad json".as_slice(),
        br#"{"version":"1.0.0","sources":{"unknown":{}}}"#,
        br#"{"version":"1.0.0","sources":{"git":{"repo":{"main":{"pin":"../bad"}}}}}"#,
    ] {
        std::fs::write(&path, bytes).unwrap();
        for mode in [RefUpdate::ReplaceComplete, RefUpdate::MergeAdditive] {
            assert!(manager.update_refs("plugin", mode, &[]).await.is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
}

#[tokio::test]
async fn failed_lockfile_saves_leave_the_old_file_and_can_retry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lock.json");
    let sys = FaultSys::default();
    let lockfile = Lockfile::load(&path, &sys).await.unwrap();
    lockfile.lock_pin("git", "repo", "a", "p1").await.unwrap();
    lockfile.save(&sys).await.unwrap();
    let old = std::fs::read(&path).unwrap();
    lockfile.lock_pin("git", "repo", "b", "p2").await.unwrap();
    for fault in [OPEN, WRITE, RENAME] {
        sys.fault.store(fault, Ordering::SeqCst);
        assert!(lockfile.save(&sys).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert_no_staging(dir.path());
    }
    sys.fault.store(0, Ordering::SeqCst);
    lockfile.save(&sys).await.unwrap();
    assert_eq!(
        Lockfile::load(&path, &sys)
            .await
            .unwrap()
            .pin("git", "repo", "b")
            .await
            .as_deref(),
        Some("p2")
    );
}

#[tokio::test]
async fn lockfile_publication_uses_the_injected_memory_filesystem() {
    let sys = system_traits::impls::InMemorySys::default();
    let path = Path::new("/workspace/lock.json");
    sys.fs_create_dir_all_async(path.parent().unwrap())
        .await
        .unwrap();
    let lockfile = Lockfile::load(path, &sys).await.unwrap();
    lockfile.lock_pin("git", "repo", "a", "p1").await.unwrap();
    lockfile.save(&sys).await.unwrap();
    let reloaded = Lockfile::load(path, &sys).await.unwrap();
    assert_eq!(
        reloaded.pin("git", "repo", "a").await.as_deref(),
        Some("p1")
    );
    assert_eq!(
        sys.fs_read_dir_async(path.parent().unwrap()).await.unwrap(),
        [path]
    );
}

#[tokio::test]
async fn concurrent_updates_union_and_reconcile_ignores_crashed_staging() {
    let dir = tempfile::tempdir().unwrap();
    let first = RemoteSourceManager::new(config(dir.path()), RealSys)
        .await
        .unwrap();
    let second = RemoteSourceManager::new(config(dir.path()), RealSys)
        .await
        .unwrap();
    let a = [reference("a", "p1")];
    let b = [reference("b", "p2")];
    let (a, b) = timeout(WAIT, async {
        tokio::join!(
            first.update_refs("plugin", RefUpdate::MergeAdditive, &a),
            second.update_refs("plugin", RefUpdate::MergeAdditive, &b),
        )
    })
    .await
    .unwrap();
    a.unwrap();
    b.unwrap();
    assert_eq!(read_refs(dir.path(), "plugin").len(), 2);
    std::fs::write(dir.path().join("refs/.omni-atomic-crashed.tmp"), b"{")
        .unwrap();
    let c = [reference("c", "p3")];
    let (a, b) = timeout(WAIT, async {
        tokio::join!(
            first.update_refs("plugin", RefUpdate::MergeAdditive, &c),
            second.reconcile_pins(),
        )
    })
    .await
    .unwrap();
    a.unwrap();
    b.unwrap();
    assert_eq!(read_refs(dir.path(), "plugin").len(), 3);
}

#[tokio::test]
async fn cancellation_keeps_workspace_lock_through_publication() {
    let dir = tempfile::tempdir().unwrap();
    let sys = FaultSys::default();
    let manager = RemoteSourceManager::new(config(dir.path()), sys.clone())
        .await
        .unwrap();
    manager
        .record_refs("plugin", &[reference("before", "p0")])
        .await
        .unwrap();
    sys.fault.store(PAUSE_RENAME, Ordering::SeqCst);
    let task = tokio::spawn(async move {
        manager
            .update_refs(
                "plugin",
                RefUpdate::ReplaceComplete,
                &[reference("a", "p1")],
            )
            .await
    });
    timeout(WAIT, sys.entered.notified()).await.unwrap();
    assert_eq!(
        read_refs(dir.path(), "plugin"),
        [("before".into(), "p0".into())]
    );
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let lockpath = dir.path().join("lock.json.lock");
    assert!(
        LockGuard::try_acquire_exclusive(&lockpath)
            .await
            .unwrap()
            .is_none()
    );
    {
        let (mutex, ready) = &*sys.release;
        *mutex.lock().unwrap() = true;
        ready.notify_all();
    }
    let _guard = timeout(WAIT, LockGuard::acquire_exclusive(lockpath))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read_refs(dir.path(), "plugin"), [("a".into(), "p1".into())]);
}

#[tokio::test]
async fn cancelled_lockfile_save_keeps_the_workspace_lock_until_commit() {
    let dir = tempfile::tempdir().unwrap();
    let sys = FaultSys::default();
    let path = dir.path().join("lock.json");
    let lockfile = Lockfile::load(&path, &sys).await.unwrap();
    lockfile.lock_pin("git", "repo", "a", "p1").await.unwrap();
    lockfile.save(&sys).await.unwrap();
    let old = std::fs::read(&path).unwrap();
    lockfile.lock_pin("git", "repo", "b", "p2").await.unwrap();
    let lockpath = dir.path().join("lock.json.lock");
    let guard = LockGuard::acquire_exclusive(&lockpath).await.unwrap();
    sys.fault.store(PAUSE_RENAME, Ordering::SeqCst);
    let writer = sys.clone();
    let task =
        tokio::spawn(async move { lockfile.save_locked(&writer, guard).await });
    timeout(WAIT, sys.entered.notified()).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), old);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        LockGuard::try_acquire_exclusive(&lockpath)
            .await
            .unwrap()
            .is_none()
    );
    {
        let (mutex, ready) = &*sys.release;
        *mutex.lock().unwrap() = true;
        ready.notify_all();
    }
    let _guard = timeout(WAIT, LockGuard::acquire_exclusive(lockpath))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        Lockfile::load(&path, &sys)
            .await
            .unwrap()
            .pin("git", "repo", "b")
            .await
            .as_deref(),
        Some("p2")
    );
}

#[tokio::test]
async fn cancelled_reconcile_cleans_memory_before_the_manager_is_reused() {
    let dir = tempfile::tempdir().unwrap();
    let sys = FaultSys::default();
    let driver = Arc::new(CountingDriver::default());
    let mut drivers = DriverRegistry::default();
    drivers.register(driver.clone());
    let manager = Arc::new(
        RemoteSourceManager::with_drivers(
            config(dir.path()),
            sys.clone(),
            drivers,
        )
        .await
        .unwrap(),
    );
    let source = reference("main", "p1").source;
    manager.materialize(&source).await.unwrap();
    manager.persist_pins().await.unwrap();
    sys.fault.store(PAUSE_RENAME, Ordering::SeqCst);
    let worker = Arc::clone(&manager);
    let task = tokio::spawn(async move { worker.reconcile_pins().await });
    timeout(WAIT, sys.entered.notified()).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    {
        let (mutex, ready) = &*sys.release;
        *mutex.lock().unwrap() = true;
        ready.notify_all();
    }
    let guard = timeout(
        WAIT,
        LockGuard::acquire_exclusive(dir.path().join("lock.json.lock")),
    )
    .await
    .unwrap()
    .unwrap();
    let spec = source.spec();
    assert!(
        Lockfile::load(dir.path().join("lock.json"), &RealSys)
            .await
            .unwrap()
            .pin("git", &spec.locator, &spec.selector)
            .await
            .is_none()
    );
    assert!(manager.locked_pin(&source).await.is_none());
    drop(guard);
    sys.fault.store(0, Ordering::SeqCst);
    // The tree is still complete, but reconciliation must force re-resolve
    // and re-record rather than using the removed pin's fast path.
    manager.materialize(&source).await.unwrap();
    assert_eq!(driver.0.load(Ordering::SeqCst), 2);
    manager.persist_pins().await.unwrap();
    assert_eq!(
        Lockfile::load(dir.path().join("lock.json"), &RealSys)
            .await
            .unwrap()
            .pin("git", &spec.locator, &spec.selector)
            .await
            .as_deref(),
        Some("p1")
    );
}

#[tokio::test]
async fn cancelled_persist_does_not_replay_saved_changes_over_a_newer_pin() {
    let dir = tempfile::tempdir().unwrap();
    let sys = FaultSys::default();
    let mut drivers = DriverRegistry::default();
    drivers.register(Arc::new(CountingDriver::default()));
    let manager = Arc::new(
        RemoteSourceManager::with_drivers(
            config(dir.path()),
            sys.clone(),
            drivers,
        )
        .await
        .unwrap(),
    );
    let source = reference("main", "p1").source;
    manager.materialize(&source).await.unwrap();
    sys.fault.store(PAUSE_RENAME, Ordering::SeqCst);
    let worker = Arc::clone(&manager);
    let task = tokio::spawn(async move { worker.persist_pins().await });
    timeout(WAIT, sys.entered.notified()).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    {
        let (mutex, ready) = &*sys.release;
        *mutex.lock().unwrap() = true;
        ready.notify_all();
    }
    let lockpath = dir.path().join("lock.json.lock");
    let guard = timeout(WAIT, LockGuard::acquire_exclusive(lockpath))
        .await
        .unwrap()
        .unwrap();
    let path = dir.path().join("lock.json");
    let spec = source.spec();
    let peer = Lockfile::load(&path, &RealSys).await.unwrap();
    peer.lock_pin("git", &spec.locator, &spec.selector, "p2")
        .await
        .unwrap();
    peer.save(&RealSys).await.unwrap();
    drop(guard);
    sys.fault.store(0, Ordering::SeqCst);
    manager.persist_pins().await.unwrap();
    assert_eq!(
        Lockfile::load(&path, &RealSys)
            .await
            .unwrap()
            .pin("git", &spec.locator, &spec.selector)
            .await
            .as_deref(),
        Some("p2")
    );
}

#[tokio::test]
async fn failed_reconcile_preserves_the_managers_pin_and_disk_file() {
    let dir = tempfile::tempdir().unwrap();
    let sys = FaultSys::default();
    let mut drivers = DriverRegistry::default();
    drivers.register(Arc::new(CountingDriver::default()));
    let manager = RemoteSourceManager::with_drivers(
        config(dir.path()),
        sys.clone(),
        drivers,
    )
    .await
    .unwrap();
    let source = reference("main", "p1").source;
    manager.materialize(&source).await.unwrap();
    manager.persist_pins().await.unwrap();
    let path = dir.path().join("lock.json");
    let old = std::fs::read(&path).unwrap();
    for fault in [OPEN, WRITE, RENAME] {
        sys.fault.store(fault, Ordering::SeqCst);
        assert!(manager.reconcile_pins().await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert_eq!(manager.locked_pin(&source).await.as_deref(), Some("p1"));
    }
}

#[tokio::test]
async fn independent_processes_merge_without_losing_refs() {
    let dir = tempfile::tempdir().unwrap();
    let mut children = Vec::new();
    for index in 0..4 {
        children.push(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "atomic_refs_process_helper",
                    "--test-threads=1",
                ])
                .env("OMNI_ATOMIC_REFS_ROOT", dir.path())
                .env("OMNI_ATOMIC_REFS_SELECTOR", format!("selector-{index}"))
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(
            timeout(WAIT, child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
    assert_eq!(read_refs(dir.path(), "plugin").len(), 4);
}

#[tokio::test]
async fn atomic_refs_process_helper() {
    let Some(root) = std::env::var_os("OMNI_ATOMIC_REFS_ROOT") else {
        return;
    };
    let selector = std::env::var("OMNI_ATOMIC_REFS_SELECTOR").unwrap();
    let manager = RemoteSourceManager::new(config(Path::new(&root)), RealSys)
        .await
        .unwrap();
    manager
        .update_refs(
            "plugin",
            RefUpdate::MergeAdditive,
            &[reference(&selector, "p1")],
        )
        .await
        .unwrap();
}
