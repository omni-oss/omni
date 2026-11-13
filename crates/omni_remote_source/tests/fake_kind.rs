//! A second, test-only source kind driven through the store core, proving a
//! kind plugs in without changes to the core, lockfile, refs or prune.
//!
//! The `fake` kind's locator is a local directory of snapshot subdirectories
//! and its selector a snapshot name. It resolves to a hash of the snapshot's
//! files and exports by copying them.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use omni_remote_source::{
    RemoteSourceRef,
    driver::{
        Connection, DriverContext, DriverRegistry, SourceDriver, SourceSpec,
    },
    error::Error,
    manager::{
        RemoteSourceManager, StorePruneFilter, config::RemoteSourceConfig,
    },
};
use system_traits::impls::RealSys;
use tempfile::TempDir;

#[derive(Default)]
struct FakeDriver {
    exports: AtomicUsize,
    resolves: AtomicUsize,
    /// Return this pin instead of the content hash.
    pin_override: Mutex<Option<String>>,
    /// Use this slug instead of the default.
    slug_override: Mutex<Option<String>>,
    /// Fail after writing one file.
    fail_export: Mutex<bool>,
}

fn snapshot_hash(dir: &Path) -> std::io::Result<String> {
    let mut files: Vec<_> = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    files.sort();
    let mut hasher = blake3::Hasher::new();
    for file in files {
        hasher.update(file.file_name().unwrap().as_encoded_bytes());
        hasher.update(&std::fs::read(&file)?);
    }
    Ok(hasher.finalize().to_hex()[..16].to_string())
}

#[async_trait::async_trait]
impl SourceDriver<RealSys> for FakeDriver {
    fn kind(&self) -> &'static str {
        "fake"
    }

    fn slug(&self, spec: &SourceSpec) -> String {
        self.slug_override
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| {
                Path::new(&spec.locator)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .trim_start_matches('.')
                    .to_lowercase()
            })
    }

    async fn resolve(
        &self,
        ctx: &DriverContext<'_, RealSys>,
        spec: &SourceSpec,
        _locked: Option<&str>,
    ) -> Result<String, Error> {
        self.resolves.fetch_add(1, Ordering::SeqCst);
        if let Some(pin) = self.pin_override.lock().unwrap().clone() {
            return Ok(pin);
        }
        let snapshot = Path::new(spec.connection.expose()).join(&spec.selector);
        let pin = snapshot_hash(&snapshot).map_err(|e| {
            Error::custom(format!(
                "fake: cannot resolve {} `{}`: {e}",
                spec.locator, spec.selector
            ))
        })?;
        // Keep a copy in the raw cache, as a real driver would.
        let raw = ctx.raw_dir.join(&pin);
        std::fs::create_dir_all(&raw)?;
        for entry in std::fs::read_dir(&snapshot)? {
            let entry = entry?;
            std::fs::copy(entry.path(), raw.join(entry.file_name()))?;
        }
        Ok(pin)
    }

    async fn export(
        &self,
        ctx: &DriverContext<'_, RealSys>,
        _spec: &SourceSpec,
        pin: &str,
        dest: &Path,
    ) -> Result<(), Error> {
        self.exports.fetch_add(1, Ordering::SeqCst);
        let raw = ctx.raw_dir.join(pin);
        for entry in std::fs::read_dir(&raw)? {
            let entry = entry?;
            std::fs::copy(entry.path(), dest.join(entry.file_name()))?;
            if *self.fail_export.lock().unwrap() {
                return Err(Error::custom("fake: export failed halfway"));
            }
        }
        Ok(())
    }
}

/// A directory of snapshots `s1`, `s2`, each with one file.
fn snapshots() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [("s1", "one"), ("s2", "two")] {
        std::fs::create_dir_all(dir.path().join(name)).unwrap();
        std::fs::write(dir.path().join(name).join("file.txt"), body).unwrap();
    }
    dir
}

fn spec(snapshots: &Path, selector: &str) -> SourceSpec {
    let locator = snapshots.to_string_lossy().into_owned();
    SourceSpec {
        connection: Connection::new(locator.clone()),
        locator,
        selector: selector.to_string(),
    }
}

struct Workspace {
    _dir: TempDir,
    sources_root: PathBuf,
    store_root: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let sources_root = dir.path().join(".omni/sources");
        std::fs::create_dir_all(&sources_root).unwrap();
        let store_root = sources_root.join("store");
        Workspace {
            _dir: dir,
            sources_root,
            store_root,
        }
    }

    fn with_store(store_root: &Path) -> Self {
        Workspace {
            store_root: store_root.to_path_buf(),
            ..Workspace::new()
        }
    }

    fn lockfile(&self) -> PathBuf {
        self.sources_root.join("lock.json")
    }

    fn config(&self) -> RemoteSourceConfig {
        RemoteSourceConfig::builder()
            .lockfile_path(self.lockfile())
            .store_root_path(self.store_root.clone())
            .build()
    }

    async fn manager(
        &self,
        driver: &Arc<FakeDriver>,
    ) -> Result<RemoteSourceManager<RealSys>, Error> {
        let mut drivers = DriverRegistry::builtin();
        drivers.register(driver.clone());
        RemoteSourceManager::with_drivers(self.config(), RealSys, drivers).await
    }

    async fn git_only_manager(
        &self,
    ) -> Result<RemoteSourceManager<RealSys>, Error> {
        RemoteSourceManager::new(self.config(), RealSys).await
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p.clone());
            }
            out.push(p);
        }
    }
    out.sort();
    out
}

#[tokio::test]
async fn a_fake_kind_materializes_locks_records_refs_and_prunes() {
    let snaps = snapshots();
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    let manager = ws.manager(&driver).await.unwrap();
    let spec = spec(snaps.path(), "s1");

    let mat = manager.materialize_with("fake", &spec).await.unwrap();
    let slug = snaps
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .trim_start_matches('.')
        .to_lowercase();
    assert_eq!(
        mat.root,
        ws.store_root
            .join("trees")
            .join("fake")
            .join(&slug)
            .join(&mat.pin)
    );
    assert_eq!(
        std::fs::read_to_string(mat.root.join("file.txt")).unwrap(),
        "one"
    );

    // Locked: a second materialization takes the fast path.
    let again = manager.materialize_with("fake", &spec).await.unwrap();
    assert_eq!(again, mat);
    assert_eq!(driver.resolves.load(Ordering::SeqCst), 1);

    manager.persist_pins().await.unwrap();
    let lock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.lockfile()).unwrap()).unwrap();
    assert_eq!(
        lock["sources"]["fake"][&spec.locator]["s1"]["pin"],
        serde_json::Value::String(mat.pin.clone())
    );

    // Refs record the kind; reconcile keeps the pin they reference.
    manager
        .record_refs("test", &[])
        .await
        .expect("record empty refs");
    let refs =
        std::fs::read_to_string(ws.sources_root.join("refs").join("test.json"))
            .unwrap();
    assert!(refs.contains("\"sources\""), "{refs}");

    // Prune lists the entry with its kind and removes the raw cache with the
    // last tree.
    let all = StorePruneFilter {
        all: true,
        ..Default::default()
    };
    let listed = manager.prune_store(&all, true).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].kind, "fake");
    assert_eq!(listed[0].remote, slug);
    assert_eq!(listed[0].pin, mat.pin);

    let only = vec![("fake".to_string(), slug.clone(), mat.pin.clone())];
    let removed = manager
        .prune_store(
            &StorePruneFilter {
                all: true,
                only: Some(&only),
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert!(!mat.root.exists());
    assert!(!ws.store_root.join("raw").join("fake").join(&slug).exists());
}

#[tokio::test]
async fn reconcile_keeps_fake_pins_referenced_by_refs() {
    let snaps = snapshots();
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    let manager = ws.manager(&driver).await.unwrap();
    let s1 = spec(snaps.path(), "s1");
    let s2 = spec(snaps.path(), "s2");
    manager.materialize_with("fake", &s1).await.unwrap();
    manager.materialize_with("fake", &s2).await.unwrap();

    // A refs file written by hand, since `RemoteSource` has no fake variant.
    std::fs::create_dir_all(ws.sources_root.join("refs")).unwrap();
    std::fs::write(
        ws.sources_root.join("refs").join("test.json"),
        serde_json::json!({
            "version": "1.0.0",
            "sources": {"fake": {s1.locator.clone(): {"s1": {"pin": "x"}}}}
        })
        .to_string(),
    )
    .unwrap();
    manager.reconcile_pins().await.unwrap();

    let lock: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.lockfile()).unwrap()).unwrap();
    let fake = &lock["sources"]["fake"][&s1.locator];
    assert!(fake.get("s1").is_some());
    assert!(fake.get("s2").is_none(), "{lock}");
}

#[tokio::test]
async fn concurrent_workspaces_export_once() {
    let snaps = snapshots();
    let shared = tempfile::tempdir().unwrap();
    let store = shared.path().join("store");
    let p = Workspace::with_store(&store);
    let u = Workspace::with_store(&store);
    let driver = Arc::new(FakeDriver::default());
    let mp = p.manager(&driver).await.unwrap();
    let mu = u.manager(&driver).await.unwrap();
    let spec = spec(snaps.path(), "s1");

    let (a, b) = tokio::join!(
        mp.materialize_with("fake", &spec),
        mu.materialize_with("fake", &spec)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a, b);
    assert_eq!(driver.exports.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn equal_slugs_of_different_kinds_never_share_a_lock_or_tree() {
    let snaps = snapshots();
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    // The same string a git remote could use as its slug.
    *driver.slug_override.lock().unwrap() = Some("shared".to_string());
    let manager = ws.manager(&driver).await.unwrap();

    let mat = manager
        .materialize_with("fake", &spec(snaps.path(), "s1"))
        .await
        .unwrap();
    assert!(
        mat.root
            .starts_with(ws.store_root.join("trees/fake/shared"))
    );
    assert!(ws.store_root.join("locks/fake/shared.lock").exists());
    assert!(!ws.store_root.join("trees/git/shared").exists());
    assert!(!ws.store_root.join("locks/git/shared.lock").exists());
}

#[tokio::test]
async fn old_layout_entries_are_neither_listed_nor_pruned() {
    let ws = Workspace::new();
    let old_tree = ws.store_root.join("trees/https_3a_2f_2fhost_2fr/c1");
    let old_mirror = ws.store_root.join("git/https_3a_2f_2fhost_2fr.git");
    std::fs::create_dir_all(&old_tree).unwrap();
    std::fs::create_dir_all(&old_mirror).unwrap();
    std::fs::write(old_tree.join("a"), "a").unwrap();

    let manager = ws.git_only_manager().await.unwrap();
    let all = StorePruneFilter {
        all: true,
        ..Default::default()
    };
    assert!(manager.prune_store(&all, true).await.unwrap().is_empty());
    assert!(manager.prune_store(&all, false).await.unwrap().is_empty());
    assert!(old_tree.join("a").exists());
    assert!(old_mirror.exists());
}

#[tokio::test]
async fn unsafe_pins_are_rejected_before_anything_is_written() {
    let snaps = snapshots();
    for bad in ["../x", "a/b", "A1", "", "x.manifest", ".hidden"] {
        let ws = Workspace::new();
        let driver = Arc::new(FakeDriver::default());
        *driver.pin_override.lock().unwrap() = Some(bad.to_string());
        let manager = ws.manager(&driver).await.unwrap();

        let error = manager
            .materialize_with("fake", &spec(snaps.path(), "s1"))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("fake") && error.contains("unsafe pin"),
            "{error}"
        );
        assert!(!ws.store_root.join("trees").exists(), "{bad}");
        assert_eq!(driver.exports.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn a_failed_export_leaves_no_tree_pending_dir_or_pin() {
    let snaps = snapshots();
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    *driver.fail_export.lock().unwrap() = true;
    let manager = ws.manager(&driver).await.unwrap();
    let spec = spec(snaps.path(), "s1");

    assert!(manager.materialize_with("fake", &spec).await.is_err());
    assert!(!ws.store_root.join("trees").exists());
    let pending: Vec<_> = std::fs::read_dir(ws.store_root.join(".pending"))
        .map(|d| d.flatten().collect())
        .unwrap_or_default();
    assert!(pending.is_empty(), "{pending:?}");
    manager.persist_pins().await.unwrap();
    let lock = std::fs::read_to_string(ws.lockfile()).unwrap_or_default();
    assert!(!lock.contains("fake"), "{lock}");
}

#[tokio::test]
async fn a_deleted_tree_is_re_exported_from_the_raw_cache() {
    let snaps = snapshots();
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    let manager = ws.manager(&driver).await.unwrap();
    let spec = spec(snaps.path(), "s1");
    let mat = manager.materialize_with("fake", &spec).await.unwrap();

    std::fs::remove_dir_all(ws.store_root.join("trees")).unwrap();
    let again = manager.materialize_with("fake", &spec).await.unwrap();
    assert_eq!(again, mat);
    assert_eq!(driver.exports.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_failed_resolve_names_the_kind_and_locator() {
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    let manager = ws.manager(&driver).await.unwrap();
    let missing = ws.sources_root.join("no-such-snapshots");

    let error = manager
        .materialize_with("fake", &spec(&missing, "s1"))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("fake"), "{error}");
    assert!(error.contains("no-such-snapshots"), "{error}");
}

#[tokio::test]
async fn a_locked_pin_must_not_change_during_resolve() {
    let snaps = snapshots();
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    let manager = ws.manager(&driver).await.unwrap();
    let spec = spec(snaps.path(), "s1");
    let mat = manager.materialize_with("fake", &spec).await.unwrap();
    manager.persist_pins().await.unwrap();
    let before = std::fs::read(ws.lockfile()).unwrap();

    // Lose the tree so the slow path runs, then have the driver drift.
    std::fs::remove_dir_all(ws.store_root.join("trees")).unwrap();
    *driver.pin_override.lock().unwrap() = Some("other".to_string());
    let reopened = ws.manager(&driver).await.unwrap();
    let error = reopened
        .materialize_with("fake", &spec)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&mat.pin) && error.contains("other"),
        "{error}"
    );
    assert!(!ws.store_root.join("trees").exists());
    reopened.persist_pins().await.unwrap();
    assert_eq!(std::fs::read(ws.lockfile()).unwrap(), before);
}

#[tokio::test]
async fn h_prefixed_and_unsafe_slugs_are_hashed() {
    let snaps = snapshots();
    for slug in ["h-abc", "Upper"] {
        let ws = Workspace::new();
        let driver = Arc::new(FakeDriver::default());
        *driver.slug_override.lock().unwrap() = Some(slug.to_string());
        let manager = ws.manager(&driver).await.unwrap();
        let mat = manager
            .materialize_with("fake", &spec(snaps.path(), "s1"))
            .await
            .unwrap();
        let on_disk = mat
            .root
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(on_disk.starts_with("h-") && on_disk != slug, "{on_disk}");
    }
}

#[tokio::test]
async fn load_errors_leave_the_file_unchanged() {
    let cases = [
        // (a) the old git-only shape
        r#"{"version":"1.0.0","git":{"https://h/r.git":{"main":{"commit":"c1"}}}}"#,
        // (b) an unknown entry field
        r#"{"version":"1.0.0","sources":{"git":{"https://h/r.git":{"main":{"pin":"c1","x":1}}}}}"#,
        // (c) a kind with no driver
        r#"{"version":"1.0.0","sources":{"registry":{}}}"#,
        // (d) no sources
        r#"{"version":"1.0.0"}"#,
        // an unsafe pin
        r#"{"version":"1.0.0","sources":{"git":{"https://h/r.git":{"main":{"pin":"../x"}}}}}"#,
    ];
    for doc in cases {
        let ws = Workspace::new();
        std::fs::write(ws.lockfile(), doc).unwrap();
        let error = match ws.git_only_manager().await {
            Ok(_) => panic!("loaded {doc}"),
            Err(error) => error.to_string(),
        };
        if doc.contains("registry") {
            assert!(error.contains("registry"), "{error}");
        }
        if doc.contains("../x") {
            assert!(error.contains("../x"), "{error}");
        }
        assert_eq!(std::fs::read_to_string(ws.lockfile()).unwrap(), doc);
    }
}

#[tokio::test]
async fn refs_naming_an_unknown_kind_fail_reconcile_without_writing() {
    let ws = Workspace::new();
    let manager = ws.git_only_manager().await.unwrap();
    std::fs::create_dir_all(ws.sources_root.join("refs")).unwrap();
    let refs = ws.sources_root.join("refs").join("x.json");
    let doc = r#"{"version":"1.0.0","sources":{"registry":{}}}"#;
    std::fs::write(&refs, doc).unwrap();

    let error = manager.reconcile_pins().await.unwrap_err().to_string();
    assert!(error.contains("registry"), "{error}");
    assert_eq!(std::fs::read_to_string(&refs).unwrap(), doc);
    assert!(!ws.lockfile().exists());
}

#[tokio::test]
async fn a_newer_store_layout_is_refused() {
    let snaps = snapshots();
    let ws = Workspace::new();
    std::fs::create_dir_all(&ws.store_root).unwrap();
    std::fs::write(ws.store_root.join("VERSION"), "2\n").unwrap();
    let driver = Arc::new(FakeDriver::default());
    let manager = ws.manager(&driver).await.unwrap();

    assert!(
        manager
            .materialize_with("fake", &spec(snaps.path(), "s1"))
            .await
            .is_err()
    );
    let written: Vec<_> = files_under(&ws.store_root)
        .into_iter()
        .filter(|p| !p.starts_with(ws.store_root.join("locks")))
        .collect();
    assert_eq!(written, vec![ws.store_root.join("VERSION")]);
}

#[tokio::test]
async fn record_refs_writes_the_kind_neutral_shape() {
    let ws = Workspace::new();
    let manager = ws.git_only_manager().await.unwrap();
    let source = omni_remote_source::RemoteSource::Git {
        uri: url::Url::parse("https://user:token@HOST/r.git/").unwrap(),
        rev: "main".to_string(),
    };
    manager
        .record_refs(
            "t",
            &[RemoteSourceRef {
                source,
                pin: "c1".to_string(),
            }],
        )
        .await
        .unwrap();
    let refs: serde_json::Value = serde_json::from_slice(
        &std::fs::read(ws.sources_root.join("refs/t.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        refs["sources"]["git"]["https://host/r.git"]["main"]["pin"],
        "c1"
    );
    assert!(!refs.to_string().contains("token"));
}

#[tokio::test]
async fn persisting_never_overwrites_a_pin_another_process_changed() {
    let snaps = snapshots();
    let ws = Workspace::new();
    let driver = Arc::new(FakeDriver::default());
    let s1 = spec(snaps.path(), "s1");
    let s2 = spec(snaps.path(), "s2");
    let lock = |pin: &str| {
        serde_json::json!({
            "version": "1.0.0",
            "sources": {"fake": {s1.locator.clone(): {"s1": {"pin": pin}}}}
        })
        .to_string()
    };
    std::fs::write(ws.lockfile(), lock("old")).unwrap();

    // This manager loads `old` and only resolves s2.
    let manager = ws.manager(&driver).await.unwrap();
    manager.materialize_with("fake", &s2).await.unwrap();

    // Another process re-locks s1 meanwhile (e.g. `install --update`).
    std::fs::write(ws.lockfile(), lock("new")).unwrap();
    manager.persist_pins().await.unwrap();

    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.lockfile()).unwrap()).unwrap();
    let pins = &saved["sources"]["fake"][&s1.locator];
    assert_eq!(pins["s1"]["pin"], "new", "{saved}");
    assert!(pins.get("s2").is_some(), "{saved}");

    // A later save does not replay our already-saved s2 pin over a newer one.
    let mut edited = saved.clone();
    edited["sources"]["fake"][&s1.locator]["s2"]["pin"] = "later".into();
    std::fs::write(ws.lockfile(), edited.to_string()).unwrap();
    manager.persist_pins().await.unwrap();
    let again: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.lockfile()).unwrap()).unwrap();
    assert_eq!(again["sources"]["fake"][&s1.locator]["s2"]["pin"], "later");
}
