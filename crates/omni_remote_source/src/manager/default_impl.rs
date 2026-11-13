use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
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
    manager::config::RemoteSourceConfig,
    manager::tree,
    refs::{RefSetData, RefSetDataV1_0_0},
    source::{MaterializedSource, RemoteSource, RemoteSourceRef},
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

/// Process-wide counter making pending directory names unique even when a
/// single process publishes several checkouts at once.
static PENDING_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct RemoteSourceManager<TSys: RemoteSourceSys> {
    lockfile: Lockfile,
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
            lockfile,
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

    async fn merge_own_pins(&self, disk: &Lockfile) -> Result<(), Error> {
        for (kind, locator, selector, pin) in self.lockfile.pins().await {
            disk.lock_pin(&kind, &locator, &selector, &pin).await?;
        }
        Ok(())
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
            if self.tree_is_complete(&tree).await {
                self.mark_used(&tree).await;
                return Ok(MaterializedSource {
                    root: tree,
                    pin: pin.clone(),
                });
            }
        }

        // One resolve or export per remote at a time across every workspace
        // sharing the store; a peer that finished first leaves the tree for us.
        let _remote_guard =
            LockGuard::acquire_exclusive(self.remote_lock_path(kind, &slug))
                .await?;
        self.check_layout().await?;

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
        self.mark_used(&tree).await;

        if locked.is_none() {
            self.lockfile
                .lock_pin(kind, &spec.locator, &spec.selector, &pin)
                .await?;
        }
        Ok(MaterializedSource { root: tree, pin })
    }

    /// Persist the reference set a subsystem resolved on its last run to
    /// `refs/<id>.json`, sorted so the file has stable diffs. These files are
    /// the garbage-collection roots unioned by [`reconcile_pins`](Self::reconcile_pins).
    pub async fn record_refs(
        &self,
        id: &str,
        refs: &[RemoteSourceRef],
    ) -> Result<(), Error> {
        let mut data = RefSetDataV1_0_0::default();
        for r in refs {
            let spec = r.source.spec();
            data.insert(r.source.kind(), &spec.locator, &spec.selector, &r.pin);
        }

        let refs_dir = self.refs_dir();
        self.sys.fs_create_dir_all_async(&refs_dir).await?;
        let path = refs_dir.join(format!("{id}.json"));
        omni_file_data_serde::write_async(
            &path,
            &RefSetData::V1_0_0(data),
            &self.sys,
        )
        .await?;

        Ok(())
    }

    /// Drop lockfile pins that no persisted reference set references, and write
    /// the pruned lockfile. Runs under the exclusive advisory lock so no
    /// concurrent write races the reconcile.
    ///
    /// This never deletes from the store. The store can be shared by many
    /// workspaces, and the reference sets here belong to one of them, so it
    /// cannot tell which checkouts another workspace still needs. Removing
    /// checkouts is the explicit prune command's job.
    pub async fn reconcile_pins(&self) -> Result<(), Error> {
        let _guard =
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
        self.merge_own_pins(&disk).await?;
        Self::prune_lockfile_pins(&disk, &retained).await?;
        self.ensure_lockfile_parent().await?;
        disk.save(&self.sys).await?;

        Ok(())
    }

    /// Merge this manager's in-memory pins onto whatever is currently on disk
    /// and save the result, under the exclusive advisory lock. Additive by
    /// design: a lazy subsystem run never drops pins a sibling subsystem or a
    /// concurrent process just added, and never prunes.
    pub async fn persist_pins(&self) -> Result<(), Error> {
        let _guard =
            LockGuard::acquire_exclusive(self.advisory_lock_path()).await?;

        let disk = self.load_lockfile().await?;
        self.merge_own_pins(&disk).await?;

        self.ensure_lockfile_parent().await?;
        disk.save(&self.sys).await?;

        Ok(())
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
        let pending = self.new_pending_dir();
        self.sys.fs_create_dir_all_async(&pending).await?;
        if let Err(error) = driver.export(ctx, spec, pin, &pending).await {
            self.remove_tree(&pending).await;
            return Err(error);
        }
        let manifest = {
            let pending = pending.clone();
            tokio::task::spawn_blocking(move || tree::seal(&pending)).await
        };
        let manifest = match manifest {
            Ok(Ok(manifest)) => manifest,
            Ok(Err(error)) => {
                self.remove_tree(&pending).await;
                return Err(error.into());
            }
            Err(error) => {
                self.remove_tree(&pending).await;
                return Err(Error::custom(error.to_string()));
            }
        };

        if self.sys.fs_exists_no_err_async(tree).await {
            // Incomplete (a complete one was handled by the caller).
            self.remove_tree(tree).await;
        }
        if let Some(parent) = tree.parent() {
            self.sys.fs_create_dir_all_async(parent).await?;
        }
        if let Err(error) = self.sys.fs_rename_async(&pending, tree).await {
            self.remove_tree(&pending).await;
            return Err(error.into());
        }
        // Published last: until it exists the tree reads as incomplete.
        let staging = self.new_pending_dir();
        if let Err(error) = self.sys.fs_write_async(&staging, &manifest).await {
            let _ = self.sys.fs_remove_file_async(&staging).await;
            return Err(error.into());
        }
        if let Err(error) = self
            .sys
            .fs_rename_async(&staging, manifest_path(tree))
            .await
        {
            let _ = self.sys.fs_remove_file_async(&staging).await;
            return Err(error.into());
        }
        Ok(())
    }

    /// Delete a tree and its sibling markers.
    async fn remove_tree(&self, tree: &Path) {
        let _ = self.sys.fs_remove_dir_all_async(tree).await;
        let _ = self.sys.fs_remove_file_async(manifest_path(tree)).await;
        let _ = self.sys.fs_remove_file_async(used_marker(tree)).await;
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
            Ok(text) => match text.trim().parse::<u32>() {
                Ok(found) if found > LAYOUT_VERSION => {
                    Err(Error::custom(format!(
                        "the remote-source store at {} uses layout {found}, newer than this omni supports ({LAYOUT_VERSION}); upgrade omni or choose another store with --sources-dir",
                        self.store_root_path.display()
                    )))
                }
                Ok(_) => Ok(()),
                Err(_) => Err(Error::custom(format!(
                    "unrecognized store layout marker in {}",
                    path.display()
                ))),
            },
            Err(error)
                if create && error.kind() == std::io::ErrorKind::NotFound =>
            {
                self.sys
                    .fs_create_dir_all_async(&self.store_root_path)
                    .await?;
                self.sys
                    .fs_write_async(
                        &path,
                        format!("{LAYOUT_VERSION}\n").as_bytes(),
                    )
                    .await?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
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
        self.lockfile.save(&self.sys).await?;
        Ok(())
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

    fn new_pending_dir(&self) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        let seq = PENDING_COUNTER.fetch_add(1, Ordering::Relaxed);

        self.store_root_path
            .join(omni_constants::PENDING_SEGMENT)
            .join(format!("{pid}-{nanos}-{seq}"))
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
    /// `dry_run`. This is the only operation that removes checkouts from the
    /// store: nothing else deletes, so a workspace never loses what another
    /// needs. Each remote's entries are removed under that remote's lock, so a
    /// concurrent fetch of the same remote waits instead of racing the delete.
    /// A deleted checkout is refetched the next time it is used.
    pub async fn prune_store(
        &self,
        filter: &StorePruneFilter<'_>,
        dry_run: bool,
    ) -> Result<Vec<StoreEntry>, Error> {
        let now = SystemTime::now();
        let selected: Vec<StoreEntry> = self
            .store_entries()
            .await?
            .into_iter()
            .filter(|entry| filter.matches(entry, now))
            .collect();
        if dry_run {
            return Ok(selected);
        }

        let mut removed = Vec::new();
        for entry in selected {
            let lock = self.remote_lock_path(&entry.kind, &entry.remote);
            let _guard = LockGuard::acquire_exclusive(lock).await?;
            // Another process may have removed it while we waited.
            if self.sys.fs_exists_no_err_async(&entry.dir).await {
                self.remove_tree(&entry.dir).await;
                tracing::debug!("pruned store checkout: {:?}", entry.dir);
            }
            if let Some(parent) = entry.dir.parent() {
                // Once a remote's last tree is gone its raw cache has
                // nothing left to serve, so drop it too.
                if matches!(
                    self.sys.fs_read_dir_async(parent).await.map(|d| d.len()),
                    Ok(0)
                ) {
                    let _ = self.sys.fs_remove_dir_all_async(parent).await;
                    let _ = self
                        .sys
                        .fs_remove_dir_all_async(
                            self.raw_dir(&entry.kind, &entry.remote),
                        )
                        .await;
                }
            }
            removed.push(entry);
        }
        Ok(removed)
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
