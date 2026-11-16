//! Prune honors checkout leases: leased entries are skipped and reported,
//! unleased ones are reclaimed in the same run, removal failures are reported
//! without leaving a tree that reads as complete, and a reader never loses a
//! tree it holds a lease on.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use omni_remote_source::{
    LeasedSource,
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

const KIND: &str = "fake";
const REMOTE: &str = "remote";

/// Resolves a selector to itself as the pin and exports two files, one nested.
#[derive(Default)]
struct Driver {
    exports: AtomicUsize,
}

#[async_trait::async_trait]
impl SourceDriver<RealSys> for Driver {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn slug(&self, _spec: &SourceSpec) -> String {
        REMOTE.to_string()
    }

    async fn resolve(
        &self,
        ctx: &DriverContext<'_, RealSys>,
        spec: &SourceSpec,
        _locked: Option<&str>,
    ) -> Result<String, Error> {
        std::fs::create_dir_all(&ctx.raw_dir)?;
        std::fs::write(ctx.raw_dir.join("cache"), b"raw")?;
        Ok(spec.selector.clone())
    }

    async fn export(
        &self,
        _ctx: &DriverContext<'_, RealSys>,
        _spec: &SourceSpec,
        pin: &str,
        dest: &Path,
    ) -> Result<(), Error> {
        self.exports.fetch_add(1, Ordering::SeqCst);
        std::fs::write(dest.join("file.txt"), pin)?;
        std::fs::create_dir_all(dest.join("sub"))?;
        std::fs::write(dest.join("sub").join("inner.txt"), pin)?;
        Ok(())
    }
}

struct Store {
    dir: TempDir,
}

impl Store {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("store")
    }

    fn tree(&self, pin: &str) -> PathBuf {
        self.root().join("trees").join(KIND).join(REMOTE).join(pin)
    }

    fn raw(&self) -> PathBuf {
        self.root().join("raw").join(KIND).join(REMOTE)
    }

    async fn manager(&self) -> RemoteSourceManager<RealSys> {
        let mut drivers = DriverRegistry::default();
        drivers.register(Arc::new(Driver::default()));
        let config = RemoteSourceConfig::builder()
            .lockfile_path(self.dir.path().join("lock.json"))
            .store_root_path(self.root())
            .build();
        RemoteSourceManager::with_drivers(config, RealSys, drivers)
            .await
            .unwrap()
    }
}

fn spec(pin: &str) -> SourceSpec {
    SourceSpec {
        locator: "locator".into(),
        selector: pin.into(),
        connection: Connection::new("locator"),
    }
}

async fn leased(
    manager: &RemoteSourceManager<RealSys>,
    pin: &str,
) -> LeasedSource {
    manager
        .materialize_leased_with(KIND, &spec(pin))
        .await
        .unwrap()
}

fn all() -> StorePruneFilter<'static> {
    StorePruneFilter {
        all: true,
        ..Default::default()
    }
}

fn pins(entries: &[omni_remote_source::manager::StoreEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.pin.as_str()).collect()
}

#[tokio::test]
async fn prune_skips_a_leased_checkout_and_reclaims_the_rest_in_one_run() {
    let store = Store::new();
    let manager = store.manager().await;
    let held = leased(&manager, "p1").await;
    manager.materialize_with(KIND, &spec("p2")).await.unwrap();

    let report = manager.prune_store_report(&all(), false).await.unwrap();
    assert_eq!(pins(&report.removed), ["p2"]);
    assert_eq!(pins(&report.skipped_in_use), ["p1"]);
    assert!(report.failed.is_empty());
    assert!(held.source.root.join("file.txt").exists());
    assert!(!store.tree("p2").exists());
    // A leased tree keeps its remote's raw cache: the parent is not empty.
    assert!(store.raw().exists());

    // `prune_store` keeps its meaning: only what was removed.
    drop(held);
    let removed = manager.prune_store(&all(), false).await.unwrap();
    assert_eq!(pins(&removed), ["p1"]);
    assert!(!store.tree("p1").exists());
    assert!(!store.raw().exists());
}

#[tokio::test]
async fn a_confirmed_prune_of_a_leased_checkout_skips_it() {
    let store = Store::new();
    let manager = store.manager().await;
    let held = leased(&manager, "p1").await;
    let only = vec![(KIND.to_string(), REMOTE.to_string(), "p1".to_string())];
    let filter = StorePruneFilter {
        all: true,
        only: Some(&only),
        ..Default::default()
    };
    let report = manager.prune_store_report(&filter, false).await.unwrap();
    assert!(report.removed.is_empty());
    assert_eq!(pins(&report.skipped_in_use), ["p1"]);
    assert!(held.source.root.exists());
}

#[tokio::test]
async fn a_dry_run_reports_skips_without_creating_lease_files() {
    let store = Store::new();
    let manager = store.manager().await;
    manager.materialize_with(KIND, &spec("p1")).await.unwrap();
    let report = manager.prune_store_report(&all(), true).await.unwrap();
    assert_eq!(pins(&report.removed), ["p1"]);
    assert!(!store.root().join("leases").exists());
    assert!(store.tree("p1").exists());

    let held = leased(&manager, "p1").await;
    let report = manager.prune_store_report(&all(), true).await.unwrap();
    assert!(report.removed.is_empty());
    assert_eq!(pins(&report.skipped_in_use), ["p1"]);
    assert!(held.source.root.exists());
    drop(held);
    let report = manager.prune_store_report(&all(), true).await.unwrap();
    assert_eq!(pins(&report.removed), ["p1"]);
}

#[tokio::test]
async fn a_lease_survives_a_dropped_reference_and_pin_advance() {
    let store = Store::new();
    let manager = store.manager().await;
    let held = leased(&manager, "p1").await;
    manager.persist_pins().await.unwrap();
    // No refs reference the pin, so reconciliation drops it from the lockfile.
    manager.reconcile_pins().await.unwrap();
    assert!(
        manager
            .prune_store_report(&all(), false)
            .await
            .unwrap()
            .removed
            .is_empty()
    );
    assert!(held.source.root.join("file.txt").exists());
}

#[tokio::test]
async fn a_shared_lease_is_held_until_the_last_owner_drops() {
    let store = Store::new();
    let manager = store.manager().await;
    let held = leased(&manager, "p1").await;
    let clone = held.clone();
    drop(held);
    assert_eq!(
        pins(
            &manager
                .prune_store_report(&all(), false)
                .await
                .unwrap()
                .skipped_in_use
        ),
        ["p1"]
    );
    drop(clone);
    assert_eq!(
        pins(&manager.prune_store(&all(), false).await.unwrap()),
        ["p1"]
    );
}

#[tokio::test]
async fn a_pruned_checkout_is_republished_and_leased_again() {
    let store = Store::new();
    let manager = store.manager().await;
    drop(leased(&manager, "p1").await);
    manager.prune_store(&all(), false).await.unwrap();
    assert!(!store.tree("p1").exists());
    // The pin is still locked in memory: the fast path sees an incomplete tree
    // after taking the lease and falls back to publishing.
    let held = leased(&manager, "p1").await;
    assert!(held.source.root.join("file.txt").exists());
    assert_eq!(
        pins(
            &manager
                .prune_store_report(&all(), false)
                .await
                .unwrap()
                .skipped_in_use
        ),
        ["p1"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reader_never_loses_a_tree_it_holds_a_lease_on() {
    let store = Store::new();
    let reader = Arc::new(store.manager().await);
    let pruner = Arc::new(store.manager().await);
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Prune continuously while the reader repeatedly leases and inspects the
    // tree, so a prune lands in every gap the protocol must close.
    let prune = {
        let done = Arc::clone(&done);
        tokio::spawn(async move {
            let mut runs = 0;
            while !done.load(Ordering::SeqCst) {
                let report =
                    pruner.prune_store_report(&all(), false).await.unwrap();
                assert!(report.failed.is_empty(), "{:?}", report.failed);
                runs += 1;
                tokio::task::yield_now().await;
            }
            runs
        })
    };
    for round in 0..300 {
        let held = leased(&reader, "p1").await;
        for _ in 0..3 {
            tokio::task::yield_now().await;
            assert!(
                held.source.root.join("file.txt").exists()
                    && held.source.root.join("sub/inner.txt").exists(),
                "round {round}: tree vanished under a lease"
            );
        }
    }
    done.store(true, Ordering::SeqCst);
    assert!(prune.await.unwrap() > 0);
}

#[cfg(unix)]
mod failures {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .unwrap();
    }

    /// Root ignores directory permissions, so the failure cannot be injected.
    fn permissions_are_enforced(dir: &Path) -> bool {
        let probe = dir.join("probe");
        std::fs::create_dir(&probe).unwrap();
        set_mode(&probe, 0o555);
        let enforced = std::fs::write(probe.join("f"), b"").is_err();
        set_mode(&probe, 0o755);
        enforced
    }

    #[tokio::test]
    async fn a_failed_removal_is_reported_and_never_reads_as_complete() {
        let store = Store::new();
        if !permissions_are_enforced(store.dir.path()) {
            return;
        }
        let manager = store.manager().await;
        manager.materialize_with(KIND, &spec("p1")).await.unwrap();
        manager.materialize_with(KIND, &spec("p2")).await.unwrap();
        // A read-only subdirectory makes the recursive removal fail after the
        // other files are gone.
        let stuck = store.tree("p1").join("sub");
        set_mode(&stuck, 0o555);

        let report = manager.prune_store_report(&all(), false).await.unwrap();
        // The run continues past the failure and reclaims the other checkout.
        assert_eq!(pins(&report.removed), ["p2"]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].entry.pin, "p1");
        assert!(!report.failed[0].error.is_empty());
        assert!(report.skipped_in_use.is_empty());

        // The manifest went first, so the leftover tree is incomplete: the next
        // materialization re-publishes it instead of trusting a partial tree.
        set_mode(&stuck, 0o755);
        let again = manager.materialize_with(KIND, &spec("p1")).await.unwrap();
        assert!(again.root.join("file.txt").exists());
        assert!(again.root.join("sub/inner.txt").exists());
    }

    #[tokio::test]
    async fn an_unremovable_manifest_leaves_the_checkout_intact_and_complete() {
        let store = Store::new();
        if !permissions_are_enforced(store.dir.path()) {
            return;
        }
        let manager = store.manager().await;
        manager.materialize_with(KIND, &spec("p1")).await.unwrap();
        let parent = store.tree("p1").parent().unwrap().to_path_buf();
        set_mode(&parent, 0o555);

        let report = manager.prune_store_report(&all(), false).await.unwrap();
        set_mode(&parent, 0o755);
        assert!(report.removed.is_empty());
        assert_eq!(report.failed.len(), 1);
        // Nothing was deleted, so the tree still reads as complete.
        assert!(store.tree("p1").join("file.txt").exists());
        let exports_before =
            manager.materialize_with(KIND, &spec("p1")).await.unwrap();
        assert_eq!(exports_before.pin, "p1");
    }
}
