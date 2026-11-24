use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use omni_lockfile::{Lockfile, data::LockfileData};
use omni_utils::lock::LockGuard;

use crate::{
    driver::{
        DriverConfig, DriverContext, DriverRegistry, SourceDriver, SourceSpec,
        is_safe_segment, safe_slug,
    },
    error::Error,
    lease::{LeaseSet, SourceLease},
    manager::config::RemoteSourceConfig,
    manager::{
        pending::{PENDING_GRACE, PendingPath, reclaim},
        tree,
    },
    refs::{RefSetData, RefSetDataV1_0_0, RefSetUpdate, RefUpdate},
    source::{LeasedSource, MaterializedSource, RemoteSource, RemoteSourceRef},
    sys::RemoteSourceSys,
};
use system_traits::{FileType, FsMetadataValue as _};

const RAW_SEGMENT: &str = "raw";
const TREES_SEGMENT: &str = "trees";
const LAYOUT_VERSION_FILE: &str = "VERSION";
/// The store layout this omni reads and writes: per-kind raw caches under
/// `raw/`, plain trees under `trees/`, locks under `locks/`.
const LAYOUT_VERSION: u32 = 1;
const LOCKS_SEGMENT: &str = "locks";

/// `<pin>.used`, the sibling marker whose mtime is a checkout's last use.
fn used_marker(pin_dir: &Path) -> PathBuf {
    let mut name = pin_dir
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".used");
    pin_dir.with_file_name(name)
}

/// `<pin>.manifest`, the sibling file listing a published tree's files. It
/// lives outside the tree so it can never collide with a tracked file.
fn manifest_path(pin_dir: &Path) -> PathBuf {
    let mut name = pin_dir
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".manifest");
    pin_dir.with_file_name(name)
}

fn check_pin(kind: &str, spec: &SourceSpec, pin: &str) -> Result<(), Error> {
    if is_safe_segment(pin) {
        Ok(())
    } else {
        Err(Error::custom(format!(
            "the {kind} driver returned an unsafe pin {pin:?} for {}; a pin must match [a-z0-9._-], be 1 to 128 bytes and not start with `.`",
            spec.locator
        )))
    }
}

/// `(kind, locator, selector)`: one lockfile entry.
type PinKey = (String, String, String);

pub struct RemoteSourceManager<TSys: RemoteSourceSys> {
    lockfile: Arc<Lockfile>,
    /// Entries this manager locked (`Some`) or invalidated (`None`). Only these
    /// are written back, so a pin another process changed since we loaded is
    /// never overwritten with our stale copy.
    changed: Arc<std::sync::Mutex<BTreeMap<PinKey, Option<String>>>>,
    lockfile_path: PathBuf,
    store_root_path: PathBuf,
    drivers: DriverRegistry<TSys>,
    driver_config: DriverConfig,
    sys: TSys,
}

impl<TSys: RemoteSourceSys> RemoteSourceManager<TSys> {
    pub async fn new(
        config: RemoteSourceConfig,
        sys: TSys,
    ) -> Result<RemoteSourceManager<TSys>, Error> {
        Self::with_drivers(config, sys, DriverRegistry::builtin()).await
    }

    /// A manager with an explicit driver set, for tests that add a kind.
    #[doc(hidden)]
    pub async fn with_drivers(
        config: RemoteSourceConfig,
        sys: TSys,
        drivers: DriverRegistry<TSys>,
    ) -> Result<RemoteSourceManager<TSys>, Error> {
        let lockfile_path = config.lockfile_path;
        let lockfile = Lockfile::load(lockfile_path.clone(), &sys).await?;
        let manager = RemoteSourceManager {
            lockfile: Arc::new(lockfile),
            changed: Default::default(),
            lockfile_path,
            sys,
            store_root_path: config.store_root_path,
            drivers,
            driver_config: DriverConfig::default(),
        };
        manager.check_lockfile(&manager.lockfile).await?;
        Ok(manager)
    }

    /// Reject a lockfile naming a kind with no driver (saving would drop its
    /// pins) or holding a pin that is not a safe path segment.
    async fn check_lockfile(&self, lockfile: &Lockfile) -> Result<(), Error> {
        for kind in lockfile.kinds().await {
            self.check_kind(&kind, &self.lockfile_path)?;
        }
        for (kind, locator, selector, pin) in lockfile.pins().await {
            if !is_safe_segment(&pin) {
                return Err(Error::custom(format!(
                    "{} pins {kind} source {locator} `{selector}` to the unsafe pin {pin:?}",
                    self.lockfile_path.display()
                )));
            }
        }
        Ok(())
    }

    fn check_kind(&self, kind: &str, file: &Path) -> Result<(), Error> {
        if self.drivers.contains(kind) {
            Ok(())
        } else {
            Err(Error::custom(format!(
                "{} names the source kind `{kind}`, which this omni does not support",
                file.display()
            )))
        }
    }

    async fn load_lockfile(&self) -> Result<Lockfile, Error> {
        let disk =
            Lockfile::load(self.lockfile_path.clone(), &self.sys).await?;
        self.check_lockfile(&disk).await?;
        Ok(disk)
    }

    /// Apply this manager's unsaved changes to `disk`, returning the snapshot
    /// applied so `forget_saved` can drop it once saved.
    async fn merge_own_pins(
        &self,
        disk: &Lockfile,
    ) -> Result<BTreeMap<PinKey, Option<String>>, Error> {
        let changed = self.changed.lock().expect("pins mutex poisoned").clone();
        for ((kind, locator, selector), pin) in &changed {
            match pin {
                Some(pin) => {
                    disk.lock_pin(kind, locator, selector, pin).await?
                }
                None => disk.remove(kind, locator, selector).await?,
            }
        }
        Ok(changed)
    }

    /// Drop saved changes, so a later save never replays them over a pin
    /// another process changed since. A change recorded after the snapshot
    /// is kept.
    fn forget_saved(
        changed: &std::sync::Mutex<BTreeMap<PinKey, Option<String>>>,
        saved: BTreeMap<PinKey, Option<String>>,
    ) {
        let mut changed = changed.lock().expect("pins mutex poisoned");
        for (key, pin) in saved {
            if changed.get(&key) == Some(&pin) {
                changed.remove(&key);
            }
        }
    }

    fn record_change(&self, kind: &str, spec: &SourceSpec, pin: Option<&str>) {
        self.changed.lock().expect("pins mutex poisoned").insert(
            (
                kind.to_string(),
                spec.locator.clone(),
                spec.selector.clone(),
            ),
            pin.map(str::to_string),
        );
    }

    /// Once publication starts, caller cancellation must not skip corresponding
    /// in-memory cleanup. The owned task retains both state and the workspace
    /// lock through the save and cleanup, not just through the disk rename.
    async fn commit_pins(
        &self,
        disk: Lockfile,
        guard: LockGuard,
        saved: BTreeMap<PinKey, Option<String>>,
        retained: Option<HashSet<PinKey>>,
    ) -> Result<(), Error> {
        let lockfile = Arc::clone(&self.lockfile);
        let changed = Arc::clone(&self.changed);
        let sys = self.sys.clone();
        tokio::spawn(async move {
            let _guard = guard;
            let result = async {
                disk.save(&sys).await?;
                Self::forget_saved(&changed, saved);
                if let Some(retained) = retained {
                    // Force the next materialization of a removed pin to resolve
                    // and record it, even when its complete tree still exists.
                    Self::prune_lockfile_pins(&lockfile, &retained).await?;
                }
                Ok::<(), Error>(())
            }
            .await;
            if let Err(error) = &result {
                // An aborted caller cannot receive the detached commit's error.
                tracing::warn!(%error, "source pin publication failed");
            }
            result
        })
        .await
        .map_err(|error| Error::custom(error.to_string()))?
    }
}

impl<TSys: RemoteSourceSys> RemoteSourceManager<TSys> {
    /// The filesystem handle this manager operates through. Callers that need
    /// to read alongside materialization (for example discovering a manifest at
    /// a freshly materialized root) share the same handle.
    pub fn sys(&self) -> &TSys {
        &self.sys
    }

    /// Materialize a remote source into the shared, content-addressed store,
    /// returning its checkout root and immutable pin. A locked pin whose tree
    /// is present is served without a lock or the network; anything else is
    /// resolved and exported by the source's driver and published atomically.
    pub async fn materialize(
        &self,
        source: &RemoteSource,
    ) -> Result<MaterializedSource, Error> {
        self.materialize_with(source.kind(), &source.spec()).await
    }

    /// Materialize `spec` with the driver registered for `kind`. Exists so a
    /// test kind can be exercised without a `RemoteSource` variant.
    #[doc(hidden)]
    pub async fn materialize_with(
        &self,
        kind: &str,
        spec: &SourceSpec,
    ) -> Result<MaterializedSource, Error> {
        Ok(self.materialize_inner(kind, spec, false).await?.0)
    }

    /// Like [`materialize`](Self::materialize), but also returns a
    /// [`SourceLease`] that keeps [`prune_store`](Self::prune_store) from
    /// deleting the checkout while it is held. Hold it (or an `Arc` of it) for
    /// as long as the tree may be read; dropping it, or process exit, releases it.
    ///
    /// The lease is taken before the tree is trusted: on the fast path before the
    /// completeness check, and on the slow path while the remote lock is still
    /// held, so a prune cannot delete the tree between publication and lease. A
    /// lease protects against cooperating omni processes only; it is retention,
    /// not permission to use the source.
    pub async fn materialize_leased(
        &self,
        source: &RemoteSource,
    ) -> Result<LeasedSource, Error> {
        self.materialize_leased_with(source.kind(), &source.spec())
            .await
    }

    /// Materialize `source` and add its lease to `leases`, so the checkout stays
    /// protected from prune until the set is dropped. The tool for an operation
    /// that reads several sources and finishes with all of them.
    pub async fn materialize_into(
        &self,
        source: &RemoteSource,
        leases: &LeaseSet,
    ) -> Result<MaterializedSource, Error> {
        let leased = self.materialize_leased(source).await?;
        leases.hold(leased.lease);
        Ok(leased.source)
    }

    /// [`materialize_leased`](Self::materialize_leased) for an explicit kind.
    #[doc(hidden)]
    pub async fn materialize_leased_with(
        &self,
        kind: &str,
        spec: &SourceSpec,
    ) -> Result<LeasedSource, Error> {
        let (source, lease) = self.materialize_inner(kind, spec, true).await?;
        let lease = lease.ok_or_else(|| {
            Error::custom("a leased materialization returned no lease")
        })?;
        Ok(LeasedSource {
            source,
            lease: Arc::new(lease),
        })
    }

    async fn materialize_inner(
        &self,
        kind: &str,
        spec: &SourceSpec,
        leased: bool,
    ) -> Result<(MaterializedSource, Option<SourceLease>), Error> {
        let driver = self.drivers.get(kind).ok_or_else(|| {
            Error::custom(format!("no driver for source kind `{kind}`"))
        })?;
        let slug = safe_slug(&driver.slug(spec));
        let locked =
            self.lockfile.pin(kind, &spec.locator, &spec.selector).await;
        if let Some(pin) = &locked {
            check_pin(kind, spec, pin)?;
        }

        // Fast path: a locked pin whose tree is complete needs no lock and no
        // network.
        if let Some(pin) = &locked {
            let tree = self.tree_dir(kind, &slug, pin);
            // Lease first, then check: a prune that already won the exclusive
            // lease has deleted the tree (seen as incomplete below), and one
            // that loses it skips the entry.
            let lease = if leased {
                Some(
                    SourceLease::acquire(
                        &self.store_root_path,
                        kind,
                        &slug,
                        pin,
                    )
                    .await?,
                )
            } else {
                None
            };
            if self.tree_is_complete(&tree).await {
                self.mark_used(&tree).await;
                return Ok((
                    MaterializedSource {
                        root: tree,
                        pin: pin.clone(),
                    },
                    lease,
                ));
            }
            drop(lease);
        }

        // One resolve or export per remote at a time across every workspace
        // sharing the store; a peer that finished first leaves the tree for us.
        let _remote_guard =
            LockGuard::acquire_exclusive(self.remote_lock_path(kind, &slug))
                .await?;
        self.check_layout().await?;
        // Another task of this manager may have locked it while we waited.
        let locked =
            self.lockfile.pin(kind, &spec.locator, &spec.selector).await;
        if let Some(pin) = &locked {
            check_pin(kind, spec, pin)?;
        }

        let ctx = DriverContext {
            sys: &self.sys,
            raw_dir: self.raw_dir(kind, &slug),
            offline: false,
            config: &self.driver_config,
        };
        let pin = driver.resolve(&ctx, spec, locked.as_deref()).await?;
        check_pin(kind, spec, &pin)?;
        if let Some(expected) = &locked
            && *expected != pin
        {
            return Err(Error::custom(format!(
                "the {kind} driver resolved {} `{}` to {pin}, but the lockfile pins {expected}; run `omni remote-sources install --update` to re-lock it",
                spec.locator, spec.selector
            )));
        }

        let tree = self.tree_dir(kind, &slug, &pin);
        if !self.tree_is_complete(&tree).await {
            self.publish_tree(driver.as_ref(), &ctx, spec, &pin, &tree)
                .await?;
        }
        // Still holding the remote lock, which a prune needs to delete this
        // remote's trees, so the tree cannot disappear before the lease exists.
        let lease = if leased {
            Some(
                SourceLease::acquire(&self.store_root_path, kind, &slug, &pin)
                    .await?,
            )
        } else {
            None
        };
        self.mark_used(&tree).await;

        if locked.is_none() {
            self.lockfile
                .lock_pin(kind, &spec.locator, &spec.selector, &pin)
                .await?;
            self.record_change(kind, spec, Some(&pin));
        }
        Ok((MaterializedSource { root: tree, pin }, lease))
    }

    /// Persist the reference set a subsystem resolved on its last run to
    /// `refs/<id>.json`, sorted so the file has stable diffs. These files are
    /// the garbage-collection roots unioned by [`reconcile_pins`](Self::reconcile_pins).
    /// Compatibility wrapper for `update_refs(..., RefUpdate::ReplaceComplete, ...)`;
    /// new callers should state the update mode explicitly.
    pub async fn record_refs(
        &self,
        id: &str,
        refs: &[RemoteSourceRef],
    ) -> Result<(), Error> {
        self.update_refs(id, RefUpdate::ReplaceComplete, refs).await
    }

    /// Atomically update one subsystem's references under the workspace lock.
    ///
    /// Both modes reject corrupt or unreadable existing sets. IDs and pins must
    /// be safe path segments. Updates never modify a sibling subsystem. Once
    /// publication starts, cancellation may still commit the complete update;
    /// the publisher keeps the workspace lock until it finishes.
    pub async fn update_refs(
        &self,
        id: &str,
        mode: RefUpdate,
        refs: &[RemoteSourceRef],
    ) -> Result<(), Error> {
        self.update_ref_sets(&[RefSetUpdate { id, mode, refs }])
            .await
    }

    /// Stage all selected subsystem updates before publishing any of them.
    ///
    /// Contributor resolution must finish before calling this. Invalid or
    /// duplicate IDs, corrupt existing sets, serialization and staging failures
    /// leave every old set unchanged. After renaming starts, failure can leave a
    /// prefix replaced: each file is complete, and re-running install converges.
    /// The workspace lock covers preparation, staging and all renames, including
    /// after cancellation of the waiting caller.
    pub async fn update_ref_sets(
        &self,
        updates: &[RefSetUpdate<'_>],
    ) -> Result<(), Error> {
        let mut ids = HashSet::new();
        for update in updates {
            if !is_safe_segment(update.id) || !ids.insert(update.id) {
                return Err(Error::custom(format!(
                    "unsafe or duplicate reference set id {:?}",
                    update.id
                )));
            }
            for r in update.refs {
                check_pin(r.source.kind(), &r.source.spec(), &r.pin)?;
            }
        }
        if updates.is_empty() {
            return Ok(());
        }
        let guard =
            LockGuard::acquire_exclusive(self.advisory_lock_path()).await?;
        let refs_dir = self.refs_dir();
        let mut files = Vec::with_capacity(updates.len());
        for update in updates {
            let path = refs_dir.join(format!("{}.json", update.id));
            let bytes = self
                .prepare_ref_set(&path, update.mode, update.refs)
                .await?;
            files.push((path, bytes));
        }
        self.sys.fs_create_dir_all_async(&refs_dir).await?;
        let sys = self.sys.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let files: Vec<_> = files
                .iter()
                .map(|(path, bytes)| (path.as_path(), bytes.as_slice()))
                .collect();
            omni_utils::atomic_file::write_batch(&sys, &files)
        })
        .await
        .map_err(|error| Error::custom(error.to_string()))??;
        Ok(())
    }

    async fn prepare_ref_set(
        &self,
        path: &Path,
        mode: RefUpdate,
        refs: &[RemoteSourceRef],
    ) -> Result<Vec<u8>, Error> {
        // Only NotFound is empty; permission/read/parse failures must never
        // silently reset an existing set to a partial additive update.
        let existing = match self.sys.fs_read_async(path).await {
            Ok(bytes) => {
                let data: RefSetData = omni_file_data_serde::from_slice(
                    &bytes,
                    omni_file_data_serde::Format::Json,
                )?;
                let RefSetData::V1_0_0(data) = data;
                for (kind, locators) in &data.sources {
                    self.check_kind(kind, path)?;
                    for selectors in locators.values() {
                        for value in selectors.values() {
                            if !is_safe_segment(&value.pin) {
                                return Err(Error::custom(format!(
                                    "{} contains an unsafe reference pin",
                                    path.display()
                                )));
                            }
                        }
                    }
                }
                data
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                RefSetDataV1_0_0::default()
            }
            Err(error) => return Err(error.into()),
        };
        let mut data = match mode {
            RefUpdate::ReplaceComplete => RefSetDataV1_0_0::default(),
            RefUpdate::MergeAdditive => existing,
        };
        for r in refs {
            self.check_kind(r.source.kind(), path)?;
            let spec = r.source.spec();
            data.insert(r.source.kind(), &spec.locator, &spec.selector, &r.pin);
        }

        Ok(omni_file_data_serde::to_vec(
            &RefSetData::V1_0_0(data),
            omni_file_data_serde::Format::Json,
        )?)
    }

    /// Drop lockfile pins that no persisted reference set references, and write
    /// the pruned lockfile. Runs under the exclusive advisory lock so no
    /// concurrent write races the reconcile.
    ///
    /// This never deletes from the store. The store can be shared by many
    /// workspaces, and the reference sets here belong to one of them, so it
    /// cannot tell which checkouts another workspace still needs. Removing
    /// checkouts is the explicit prune command's job.
    ///
    /// Once the owned commit phase starts, cancelling the waiter does not cancel
    /// disk publication or the matching in-memory cleanup.
    pub async fn reconcile_pins(&self) -> Result<(), Error> {
        let guard =
            LockGuard::acquire_exclusive(self.advisory_lock_path()).await?;

        let mut retained: HashSet<(String, String, String)> = HashSet::new();

        let refs_dir = self.refs_dir();
        if self.sys.fs_exists_no_err_async(&refs_dir).await {
            for entry in self.sys.fs_read_dir_async(&refs_dir).await? {
                if entry.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }

                let data: RefSetData =
                    omni_file_data_serde::read_async(&entry, &self.sys).await?;
                let RefSetData::V1_0_0(v1) = data;
                for (kind, locators) in v1.sources {
                    self.check_kind(&kind, &entry)?;
                    for (locator, selectors) in locators {
                        for selector in selectors.into_keys() {
                            retained.insert((
                                kind.clone(),
                                locator.clone(),
                                selector,
                            ));
                        }
                    }
                }
            }
        }

        // Work on the file as it is now, not the copy loaded when this manager
        // opened: another process may have added pins since, and saving ours
        // would drop them. Our own resolved pins are merged in first.
        let disk = self.load_lockfile().await?;
        let saved = self.merge_own_pins(&disk).await?;
        Self::prune_lockfile_pins(&disk, &retained).await?;
        self.ensure_lockfile_parent().await?;
        self.commit_pins(disk, guard, saved, Some(retained)).await
    }

    /// Merge this manager's in-memory pins onto whatever is currently on disk
    /// and save the result, under the exclusive advisory lock. Additive by
    /// design: a lazy subsystem run never drops pins a sibling subsystem or a
    /// concurrent process just added, and never prunes.
    /// Once commit starts, cancellation does not skip saved-change cleanup.
    pub async fn persist_pins(&self) -> Result<(), Error> {
        let guard =
            LockGuard::acquire_exclusive(self.advisory_lock_path()).await?;

        let disk = self.load_lockfile().await?;
        let saved = self.merge_own_pins(&disk).await?;

        self.ensure_lockfile_parent().await?;
        self.commit_pins(disk, guard, saved, None).await
    }

    /// Export `pin` into `tree` through a pending directory so a reader never
    /// sees a half-written tree. Replaces an incomplete tree left by a cleaner
    /// or a crash.
    async fn publish_tree(
        &self,
        driver: &dyn SourceDriver<TSys>,
        ctx: &DriverContext<'_, TSys>,
        spec: &SourceSpec,
        pin: &str,
        tree: &Path,
    ) -> Result<(), Error> {
        match reclaim(&self.store_root_path, PENDING_GRACE, false).await {
            Ok(paths) if !paths.is_empty() => {
                tracing::debug!(
                    "reclaimed {} abandoned pending paths",
                    paths.len()
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!("could not reclaim pending paths: {error}");
            }
        }
        // Owned guards remove the paths on every exit, including cancellation.
        let pending_guard = PendingPath::create(&self.store_root_path).await?;
        let pending = pending_guard.path().to_path_buf();
        self.sys.fs_create_dir_all_async(&pending).await?;
        driver.export(ctx, spec, pin, &pending).await?;
        let manifest = {
            let pending = pending.clone();
            tokio::task::spawn_blocking(move || tree::seal(&pending)).await
        };
        let manifest = match manifest {
            Ok(Ok(manifest)) => manifest,
            Ok(Err(error)) => return Err(error.into()),
            Err(error) => return Err(Error::custom(error.to_string())),
        };

        if self.sys.fs_exists_no_err_async(tree).await {
            // Incomplete (a complete one was handled by the caller).
            self.remove_tree(tree).await;
        }
        if let Some(parent) = tree.parent() {
            self.sys.fs_create_dir_all_async(parent).await?;
        }
        self.sys.fs_rename_async(&pending, tree).await?;
        // Published last: until it exists the tree reads as incomplete.
        let staging_guard = PendingPath::create(&self.store_root_path).await?;
        let staging = staging_guard.path();
        self.sys.fs_write_async(staging, &manifest).await?;
        self.sys
            .fs_rename_async(staging, manifest_path(tree))
            .await?;
        Ok(())
    }

    /// Delete a tree and its sibling markers.
    async fn remove_tree(&self, tree: &Path) {
        let _ = self.sys.fs_remove_dir_all_async(tree).await;
        let _ = self.sys.fs_remove_file_async(manifest_path(tree)).await;
        let _ = self.sys.fs_remove_file_async(used_marker(tree)).await;
    }

    /// Delete a tree and its markers, reporting failure. Directory removal is
    /// not atomic and can fail after deleting some files, so the manifest (the
    /// completeness marker) goes first: whatever remains reads as incomplete and
    /// is never reused. If the manifest cannot be removed nothing else is touched.
    async fn remove_tree_checked(sys: &TSys, tree: &Path) -> Result<(), Error> {
        ignore_not_found(sys.fs_remove_file_async(manifest_path(tree)).await)?;
        ignore_not_found(sys.fs_remove_dir_all_async(tree).await)?;
        // A leftover marker for a tree that is gone retains nothing, so a failure
        // here is logged, not reported as a failed prune.
        if let Err(error) =
            ignore_not_found(sys.fs_remove_file_async(used_marker(tree)).await)
        {
            tracing::warn!(
                "could not remove the use marker of {tree:?}: {error}"
            );
        }
        Ok(())
    }

    async fn tree_is_complete(&self, tree: &Path) -> bool {
        let manifest = manifest_path(tree);
        let tree = tree.to_path_buf();
        tokio::task::spawn_blocking(move || tree::is_complete(&tree, &manifest))
            .await
            .unwrap_or(false)
    }

    /// Refuse to touch a store written by a newer layout; create `VERSION` for
    /// a new one.
    async fn check_layout(&self) -> Result<(), Error> {
        self.layout(true).await
    }

    /// The same compatibility guard without creating anything, for operations
    /// that only read or delete (listing, pruning): an older omni must not
    /// delete from a layout it cannot interpret.
    async fn check_layout_readonly(&self) -> Result<(), Error> {
        self.layout(false).await
    }

    async fn layout(&self, create: bool) -> Result<(), Error> {
        let path = self.store_root_path.join(LAYOUT_VERSION_FILE);
        match self.sys.fs_read_to_string_async(&path).await {
            Ok(text) => self.validate_layout_marker(&path, &text),
            Err(error)
                if create && error.kind() == std::io::ErrorKind::NotFound =>
            {
                self.initialize_layout(path).await
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    fn validate_layout_marker(
        &self,
        path: &Path,
        text: &str,
    ) -> Result<(), Error> {
        match text.trim().parse::<u32>() {
            Ok(found) if found > LAYOUT_VERSION => Err(Error::custom(format!(
                "the remote-source store at {} uses layout {found}, newer than this omni supports ({LAYOUT_VERSION}); upgrade omni or choose another store with --sources-dir",
                self.store_root_path.display()
            ))),
            Ok(_) => Ok(()),
            Err(_) => Err(Error::custom(format!(
                "unrecognized store layout marker in {}",
                path.display()
            ))),
        }
    }

    async fn initialize_layout(&self, path: PathBuf) -> Result<(), Error> {
        // Different remotes have different locks, but share this marker. Recheck
        // under a short-lived store initialization lock so a late initializer
        // cannot replace a marker a peer has already published (or downgrade it).
        // This lock is never held while acquiring a remote lock.
        let guard = LockGuard::acquire_exclusive(
            self.store_root_path.join(".layout.lock"),
        )
        .await?;
        match self.sys.fs_read_to_string_async(&path).await {
            Ok(text) => return self.validate_layout_marker(&path, &text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.sys
            .fs_create_dir_all_async(&self.store_root_path)
            .await?;
        let sys = self.sys.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            // A lock-free layout reader must see either no marker or the
            // complete marker, never the direct writer's truncate/write window.
            omni_utils::atomic_file::write(
                &sys,
                &path,
                format!("{LAYOUT_VERSION}\n").as_bytes(),
            )
        })
        .await
        .map_err(|error| Error::custom(error.to_string()))??;
        Ok(())
    }

    /// Record that a checkout was just used, for age-based pruning. The marker
    /// is a sibling `<commit>.used` file whose mtime is the last use. Failing to
    /// write it only makes the checkout look older than it is.
    async fn mark_used(&self, pin_dir: &Path) {
        let marker = used_marker(pin_dir);
        if let Err(error) = self.sys.fs_write_async(&marker, b"").await {
            tracing::debug!("could not mark {pin_dir:?} used: {error}");
        }
    }

    async fn prune_lockfile_pins(
        lockfile: &Lockfile,
        retained: &HashSet<(String, String, String)>,
    ) -> Result<(), Error> {
        lockfile
            .modify(|d| {
                match d {
                    LockfileData::V1_0_0(v1) => {
                        v1.sources.retain(|kind, locators| {
                            locators.retain(|locator, selectors| {
                                selectors.retain(|selector, _| {
                                    retained.contains(&(
                                        kind.clone(),
                                        locator.clone(),
                                        selector.clone(),
                                    ))
                                });
                                !selectors.is_empty()
                            });
                            !locators.is_empty()
                        });
                    }
                }
                Ok(())
            })
            .await?;

        Ok(())
    }
}

impl<TSys: RemoteSourceSys> RemoteSourceManager<TSys> {
    pub async fn lock(&self) -> Result<(), Error> {
        self.persist_pins().await
    }

    /// The pin currently locked for `source`, if any.
    pub async fn locked_pin(&self, source: &RemoteSource) -> Option<String> {
        let spec = source.spec();
        self.lockfile
            .pin(source.kind(), &spec.locator, &spec.selector)
            .await
    }

    /// Forget the locked pin for `source` so the next materialization
    /// re-resolves it. Used to advance a mutable ref (e.g. a branch) on an
    /// explicit update.
    pub async fn invalidate(&self, source: &RemoteSource) -> Result<(), Error> {
        let spec = source.spec();
        self.lockfile
            .remove(source.kind(), &spec.locator, &spec.selector)
            .await?;
        self.record_change(source.kind(), &spec, None);
        Ok(())
    }

    /// `trees/<kind>/<slug>/<pin>`: the plain export of one pin.
    fn tree_dir(&self, kind: &str, slug: &str, pin: &str) -> PathBuf {
        self.store_root_path
            .join(TREES_SEGMENT)
            .join(kind)
            .join(slug)
            .join(pin)
    }

    /// `raw/<kind>/<slug>`: the driver's cache for one remote (git: the bare
    /// mirror), shared by every pin.
    fn raw_dir(&self, kind: &str, slug: &str) -> PathBuf {
        self.store_root_path.join(RAW_SEGMENT).join(kind).join(slug)
    }

    fn remote_lock_path(&self, kind: &str, slug: &str) -> PathBuf {
        self.store_root_path
            .join(LOCKS_SEGMENT)
            .join(kind)
            .join(format!("{slug}.lock"))
    }

    fn refs_dir(&self) -> PathBuf {
        match self.lockfile_path.parent() {
            Some(parent) => parent.join(omni_constants::REFS_SEGMENT),
            None => PathBuf::from(omni_constants::REFS_SEGMENT),
        }
    }

    fn advisory_lock_path(&self) -> PathBuf {
        let mut name = self
            .lockfile_path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| {
                OsString::from(omni_constants::SOURCE_LOCKFILE_NAME)
            });
        name.push(".lock");

        match self.lockfile_path.parent() {
            Some(parent) => parent.join(name),
            None => PathBuf::from(name),
        }
    }

    async fn ensure_lockfile_parent(&self) -> Result<(), Error> {
        if let Some(parent) = self.lockfile_path.parent() {
            self.sys.fs_create_dir_all_async(parent).await?;
        }
        Ok(())
    }
}

/// One checkout in the store, as reported by
/// [`RemoteSourceManager::store_entries`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreEntry {
    /// The source kind, e.g. `git`.
    pub kind: String,
    /// The remote's directory name in the store.
    pub remote: String,
    pub pin: String,
    pub dir: PathBuf,
    /// Total size of the files in the checkout, in bytes.
    pub size: u64,
    /// When the checkout was last used (its `.used` marker, else its own
    /// modification time).
    pub last_used: SystemTime,
}

/// What a prune did. A dry run fills `removed` with what would be removed.
#[derive(Debug, Clone, Default)]
pub struct PruneReport {
    pub removed: Vec<StoreEntry>,
    /// Checkouts a reader holds a lease on; left in place.
    pub skipped_in_use: Vec<StoreEntry>,
    /// Checkouts that could not be removed. A partial removal leaves an
    /// incomplete tree that the next publish or prune clears.
    pub failed: Vec<PruneFailure>,
    /// Abandoned `.pending` leftovers of dead publishers: removed, or for a dry
    /// run what would be.
    pub reclaimed_pending: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct PruneFailure {
    pub entry: StoreEntry,
    pub error: String,
}

fn ignore_not_found(result: std::io::Result<()>) -> Result<(), Error> {
    match result {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(error.into())
        }
        _ => Ok(()),
    }
}

/// Which store entries [`RemoteSourceManager::prune_store`] selects. An entry
/// must satisfy every filter that is set; `all` selects every entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorePruneFilter<'a> {
    pub all: bool,
    pub older_than: Option<std::time::Duration>,
    pub larger_than: Option<u64>,
    /// Restrict the selection to these `(kind, remote, pin)` entries: the
    /// checkouts a user was shown and confirmed. A checkout added since is
    /// never deleted by a confirmed prune.
    pub only: Option<&'a [(String, String, String)]>,
}

impl StorePruneFilter<'_> {
    pub fn has_criteria(&self) -> bool {
        self.all || self.older_than.is_some() || self.larger_than.is_some()
    }

    fn matches(&self, entry: &StoreEntry, now: SystemTime) -> bool {
        if !self.has_criteria() {
            return false;
        }
        if let Some(only) = self.only
            && !only.iter().any(|(kind, remote, pin)| {
                *kind == entry.kind
                    && *remote == entry.remote
                    && *pin == entry.pin
            })
        {
            return false;
        }
        if self.all {
            return true;
        }
        if let Some(age) = self.older_than {
            let used_for =
                now.duration_since(entry.last_used).unwrap_or_default();
            if used_for < age {
                return false;
            }
        }
        if let Some(size) = self.larger_than
            && entry.size <= size
        {
            return false;
        }
        true
    }
}

impl<TSys: RemoteSourceSys> RemoteSourceManager<TSys> {
    /// Every checkout in the store with its size and last use. Only the trees
    /// of registered kinds are listed; anything else under `trees/` is not
    /// ours to report or delete.
    pub async fn store_entries(&self) -> Result<Vec<StoreEntry>, Error> {
        self.check_layout_readonly().await?;

        let mut entries = Vec::new();
        for kind in self.drivers.kinds() {
            let kind_root = self.store_root_path.join(TREES_SEGMENT).join(kind);
            if !self.sys.fs_is_dir_no_err_async(&kind_root).await {
                continue;
            }
            for remote_dir in self.sys.fs_read_dir_async(&kind_root).await? {
                let Some(remote) = remote_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                else {
                    continue;
                };
                let Ok(children) =
                    self.sys.fs_read_dir_async(&remote_dir).await
                else {
                    continue;
                };
                for dir in children {
                    if !self.sys.fs_is_dir_no_err_async(&dir).await {
                        continue;
                    }
                    let Some(pin) = dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                    else {
                        continue;
                    };
                    let last_used = self.last_used(&dir).await;
                    let size = self.dir_size(&dir).await?;
                    entries.push(StoreEntry {
                        kind: kind.to_string(),
                        remote: remote.clone(),
                        pin,
                        dir,
                        size,
                        last_used,
                    });
                }
            }
        }
        entries.sort_by(|a, b| a.dir.cmp(&b.dir));
        Ok(entries)
    }

    /// Delete the checkouts `filter` selects, or only report them when
    /// `dry_run`, returning the entries removed (or that would be). Checkouts held
    /// by a [`SourceLease`] are skipped, not an error; use
    /// [`prune_store_report`](Self::prune_store_report) to see which were skipped
    /// or failed, since this returns only the removed entries.
    pub async fn prune_store(
        &self,
        filter: &StorePruneFilter<'_>,
        dry_run: bool,
    ) -> Result<Vec<StoreEntry>, Error> {
        Ok(self.prune_store_report(filter, dry_run).await?.removed)
    }

    /// Delete the checkouts `filter` selects, or only report them when
    /// `dry_run`. This is the only operation that removes checkouts from the
    /// store: nothing else deletes, so a workspace never loses what another
    /// needs. Each entry is removed under its remote's lock, so a concurrent
    /// fetch of the same remote waits instead of racing the delete, and only
    /// after taking its lease exclusively without waiting: a checkout a reader
    /// leases is skipped (`skipped_in_use`) and unrelated ones are still
    /// reclaimed. A failed removal is recorded in `failed` and the run continues.
    /// A deleted checkout is refetched the next time it is used.
    ///
    /// Once an entry's removal starts it finishes while still holding the remote
    /// lock and lease, even if the caller is cancelled.
    ///
    /// The sweep of abandoned `.pending` leftovers is store-wide and ignores
    /// the filter; a failure of the sweep is logged and does not stop pruning.
    ///
    /// A dry run is lock-free and approximate: it reports what a real run would
    /// remove or skip right now.
    pub async fn prune_store_report(
        &self,
        filter: &StorePruneFilter<'_>,
        dry_run: bool,
    ) -> Result<PruneReport, Error> {
        let now = SystemTime::now();
        let reclaimed_pending = match reclaim(
            &self.store_root_path,
            PENDING_GRACE,
            dry_run,
        )
        .await
        {
            Ok(paths) => paths,
            Err(error) => {
                tracing::warn!("could not reclaim pending paths: {error}");
                Vec::new()
            }
        };
        let selected: Vec<StoreEntry> = self
            .store_entries()
            .await?
            .into_iter()
            .filter(|entry| filter.matches(entry, now))
            .collect();

        let mut report = PruneReport {
            reclaimed_pending,
            ..Default::default()
        };
        if dry_run {
            for entry in selected {
                match SourceLease::is_held(
                    &self.store_root_path,
                    &entry.kind,
                    &entry.remote,
                    &entry.pin,
                )
                .await
                {
                    Ok(true) => report.skipped_in_use.push(entry),
                    Ok(false) => report.removed.push(entry),
                    Err(error) => report.failed.push(PruneFailure {
                        entry,
                        error: error.to_string(),
                    }),
                }
            }
            return Ok(report);
        }

        for entry in selected {
            match self.prune_entry(&entry).await {
                Ok(true) => report.removed.push(entry),
                Ok(false) => report.skipped_in_use.push(entry),
                Err(error) => {
                    tracing::warn!(
                        "could not prune store checkout {:?}: {error}",
                        entry.dir
                    );
                    report.failed.push(PruneFailure {
                        entry,
                        error: error.to_string(),
                    });
                }
            }
        }
        Ok(report)
    }

    /// Remove one entry. `Ok(false)` means a reader holds its lease.
    async fn prune_entry(&self, entry: &StoreEntry) -> Result<bool, Error> {
        // Lock order: remote lock, then the lease (never the reverse).
        let _remote = LockGuard::acquire_exclusive(
            self.remote_lock_path(&entry.kind, &entry.remote),
        )
        .await?;
        // An unsafe directory name cannot be mapped to a lease file, so it is
        // reported rather than deleted without coordination.
        let Some(_lease) = SourceLease::try_acquire_exclusive(
            &self.store_root_path,
            &entry.kind,
            &entry.remote,
            &entry.pin,
        )
        .await?
        else {
            return Ok(false);
        };
        // From here the work is destructive, so an owned task finishes it while
        // holding both guards even if the caller is cancelled. Otherwise a peer
        // could publish and lease a replacement at this path and then have it
        // deleted by the abandoned removal.
        let sys = self.sys.clone();
        let entry = entry.clone();
        let raw = self.raw_dir(&entry.kind, &entry.remote);
        tokio::spawn(async move {
            let _remote = _remote;
            let _lease = _lease;
            // Another process may have removed it while we waited.
            if sys.fs_exists_no_err_async(&entry.dir).await {
                Self::remove_tree_checked(&sys, &entry.dir).await?;
                tracing::debug!("pruned store checkout: {:?}", entry.dir);
            }
            if let Some(parent) = entry.dir.parent() {
                // Once a remote's last tree is gone its raw cache has nothing
                // left to serve, so drop it too. A leased tree still exists, so
                // it keeps the parent non-empty.
                if matches!(
                    sys.fs_read_dir_async(parent).await.map(|d| d.len()),
                    Ok(0)
                ) {
                    let _ = sys.fs_remove_dir_all_async(parent).await;
                    let _ = sys.fs_remove_dir_all_async(&raw).await;
                }
            }
            Ok::<(), Error>(())
        })
        .await
        .map_err(|error| Error::custom(error.to_string()))??;
        Ok(true)
    }

    async fn last_used(&self, commit_dir: &Path) -> SystemTime {
        for path in [used_marker(commit_dir), commit_dir.to_path_buf()] {
            if let Ok(meta) = self.sys.fs_metadata_async(&path).await
                && let Ok(modified) = meta.modified()
            {
                return modified;
            }
        }
        UNIX_EPOCH
    }

    async fn dir_size(&self, dir: &Path) -> Result<u64, Error> {
        let mut total = 0;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let children = self.sys.fs_read_dir_async(&current).await?;
            for child in children {
                // Inspect the link itself: following a directory symlink would
                // count its target twice, and `a -> .` would never end. A
                // failure is an error, not a silently smaller size, so
                // `--larger-than` never skips a checkout it could not measure.
                let meta = self.sys.fs_symlink_metadata_async(&child).await?;
                match meta.file_type() {
                    FileType::Dir => stack.push(child),
                    FileType::Symlink => {}
                    _ => total += meta.len(),
                }
            }
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_store_core_does_not_depend_on_git() {
        let core = include_str!("default_impl.rs");
        let code = &core[..core.find("#[cfg(test)]").unwrap()];
        for needle in ["omni_git_utils", "crate::git", "GitDriver"] {
            assert!(!code.contains(needle), "the core mentions {needle}");
        }
    }
}
