use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use omni_lockfile::{Lockfile, data::LockfileData};
use omni_utils::lock::LockGuard;
use url::Url;

use crate::{
    error::Error,
    manager::config::RemoteSourceConfig,
    manager::tree,
    refs::{GitRef, RefSetData, RefSetDataV1_0_0},
    source::{MaterializedSource, RemoteSource, RemoteSourceRef},
    sys::RemoteSourceSys,
};
use system_traits::{FileType, FsMetadataValue as _};

const GIT_KIND_SEGMENT: &str = "git";
const TREES_SEGMENT: &str = "trees";
const LAYOUT_VERSION_FILE: &str = "VERSION";
/// The store layout this omni reads and writes: bare mirrors under `git/`,
/// plain trees under `trees/`.
const LAYOUT_VERSION: u32 = 1;
const LOCKS_SEGMENT: &str = "locks";

/// `<commit>.used`, the sibling marker whose mtime is a checkout's last use.
fn used_marker(commit_dir: &Path) -> PathBuf {
    let mut name = commit_dir
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".used");
    commit_dir.with_file_name(name)
}

/// The store directory name of a remote, derived from its full identity:
/// scheme, host, port and path, without credentials. The encoding is injective
/// (every character other than letters, digits, `-` and `.` becomes `_` plus its
/// hex code, `_` included), so distinct remotes such as `host/a/b` and
/// `host/a_b` can never share a mirror, while the same remote with different
/// credentials does.
fn store_slug(uri: &Url) -> String {
    let mut identity = String::new();
    identity.push_str(uri.scheme());
    identity.push_str("://");
    identity.push_str(uri.host_str().unwrap_or("local"));
    if let Some(port) = uri.port() {
        identity.push_str(&format!(":{port}"));
    }
    identity.push_str(uri.path().trim_end_matches('/'));
    identity
        .trim_end_matches(".git")
        .chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                vec![c]
            } else {
                format!("_{:02x}", c as u32).chars().collect()
            }
        })
        .collect()
}

/// `<commit>.manifest`, the sibling file listing a published tree's files. It
/// lives outside the tree so it can never collide with a tracked file.
fn manifest_path(commit_dir: &Path) -> PathBuf {
    let mut name = commit_dir
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".manifest");
    commit_dir.with_file_name(name)
}

/// Process-wide counter making pending directory names unique even when a
/// single process publishes several checkouts at once.
static PENDING_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct RemoteSourceManager<TSys: RemoteSourceSys> {
    lockfile: Lockfile,
    lockfile_path: PathBuf,
    store_root_path: PathBuf,
    sys: TSys,
}

impl<TSys: RemoteSourceSys> RemoteSourceManager<TSys> {
    pub async fn new(
        config: RemoteSourceConfig,
        sys: TSys,
    ) -> Result<RemoteSourceManager<TSys>, Error> {
        let lockfile_path = config.lockfile_path;
        let lockfile = Lockfile::load(lockfile_path.clone(), &sys).await?;

        Ok(RemoteSourceManager {
            lockfile,
            lockfile_path,
            sys,
            store_root_path: config.store_root_path,
        })
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
    /// returning its checkout root and immutable pin. The git arm honors a
    /// locked commit (materializing offline when the checkout is already
    /// present) and otherwise clones into a temporary directory before
    /// atomically publishing it under the resolved commit.
    pub async fn materialize(
        &self,
        source: &RemoteSource,
    ) -> Result<MaterializedSource, Error> {
        match source {
            RemoteSource::Git { uri, rev } => {
                let (root, pin) = self.materialize_git(uri, rev).await?;
                Ok(MaterializedSource { root, pin })
            }
        }
    }

    /// Persist the reference set a subsystem resolved on its last run to
    /// `refs/<id>.json`, sorted so the file has stable diffs. These files are
    /// the garbage-collection roots unioned by [`retain`](Self::retain).
    pub async fn record_refs(
        &self,
        id: &str,
        refs: &[RemoteSourceRef],
    ) -> Result<(), Error> {
        let mut data = RefSetDataV1_0_0::default();

        for r in refs {
            match &r.source {
                RemoteSource::Git { uri, rev } => {
                    data.git.entry(uri.clone()).or_default().insert(
                        rev.clone(),
                        GitRef {
                            commit: r.pin.clone(),
                        },
                    );
                }
            }
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

        let mut retained_keys: HashSet<(Url, String)> = HashSet::new();

        let refs_dir = self.refs_dir();
        if self.sys.fs_exists_no_err_async(&refs_dir).await {
            for entry in self.sys.fs_read_dir_async(&refs_dir).await? {
                if entry.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }

                let data: RefSetData =
                    omni_file_data_serde::read_async(&entry, &self.sys).await?;

                match data {
                    RefSetData::V1_0_0(v1) => {
                        for (uri, revs) in v1.git {
                            for rev in revs.into_keys() {
                                retained_keys.insert((uri.clone(), rev));
                            }
                        }
                    }
                }
            }
        }

        // Work on the file as it is now, not the copy loaded when this manager
        // opened: another process may have added pins since, and saving ours
        // would drop them. Our own resolved pins are merged in first.
        let disk =
            Lockfile::load(self.lockfile_path.clone(), &self.sys).await?;
        for (uri, rev, commit) in self.lockfile.git_pins().await {
            disk.lock_git_commit(&uri, &rev, &commit).await?;
        }
        Self::prune_lockfile_pins(&disk, &retained_keys).await?;
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

        let disk =
            Lockfile::load(self.lockfile_path.clone(), &self.sys).await?;
        for (uri, rev, commit) in self.lockfile.git_pins().await {
            disk.lock_git_commit(&uri, &rev, &commit).await?;
        }

        self.ensure_lockfile_parent().await?;
        disk.save(&self.sys).await?;

        Ok(())
    }

    async fn materialize_git(
        &self,
        uri: &Url,
        rev: &str,
    ) -> Result<(PathBuf, String), Error> {
        let slug = store_slug(uri);
        let pinned = self.lockfile.get_git_commit(uri, rev).await;

        // Fast path: a pinned commit whose tree is complete needs no lock and
        // no network.
        if let Some(commit) = &pinned {
            let tree = self.tree_dir(&slug, commit);
            if self.tree_is_complete(&tree).await {
                self.mark_used(&tree).await;
                return Ok((tree, commit.clone()));
            }
        }

        // One fetch or export per remote at a time across every workspace
        // sharing the store; a peer that finished first leaves the tree for us.
        let _remote_guard =
            LockGuard::acquire_exclusive(self.remote_lock_path(&slug)).await?;
        self.check_layout().await?;

        let bare = self.bare_dir(&slug);
        let commit = self
            .resolve_commit(uri, &bare, pinned.as_deref(), rev)
            .await?;
        let tree = self.tree_dir(&slug, &commit);
        if !self.tree_is_complete(&tree).await {
            self.export_tree(&bare, &commit, &tree).await?;
        }
        self.mark_used(&tree).await;

        if pinned.is_none() {
            self.lockfile.lock_git_commit(uri, rev, &commit).await?;
        }
        Ok((tree, commit))
    }

    /// The commit to materialize. A pinned commit already in the mirror needs no
    /// network; anything else (a moving ref, a first use, a pin the mirror lacks)
    /// fetches first so it is resolved against the remote's current state.
    async fn resolve_commit(
        &self,
        uri: &Url,
        bare: &Path,
        pinned: Option<&str>,
        rev: &str,
    ) -> Result<String, Error> {
        if let Some(commit) = pinned
            && omni_git_utils::resolve_commit(bare, commit).as_deref()
                == Some(commit)
        {
            return Ok(commit.to_string());
        }

        omni_git_utils::fetch_bare(&self.sys, uri.as_str(), bare)
            .await
            .map_err(|error| {
                Error::custom(format!(
                    "could not fetch {} (the network is needed when the local mirror does not have `{}`): {error}",
                    omni_redact::redact_url_userinfo(uri.as_ref()),
                    pinned.unwrap_or(rev),
                ))
            })?;
        tracing::trace!(
            "fetched git repo uri: {}",
            omni_redact::redact_url_userinfo(uri.as_ref()),
        );
        let wanted = pinned.unwrap_or(rev);
        omni_git_utils::resolve_commit(bare, wanted).ok_or_else(|| {
            Error::custom(format!(
                "revision `{wanted}` was not found in {}",
                omni_redact::redact_url_userinfo(uri.as_ref())
            ))
        })
    }

    /// Export `commit` into `tree` through the pending directory so a reader
    /// never sees a half-written tree. Replaces an incomplete tree left by a
    /// cleaner or a crash.
    async fn export_tree(
        &self,
        bare: &Path,
        commit: &str,
        tree: &Path,
    ) -> Result<(), Error> {
        let pending = self.new_pending_dir();
        omni_git_utils::export_tree(&self.sys, bare, commit, &pending).await?;
        let manifest = {
            let pending = pending.clone();
            tokio::task::spawn_blocking(move || tree::seal(&pending))
                .await
                .map_err(|e| Error::custom(e.to_string()))?
        };
        let manifest = match manifest {
            Ok(manifest) => manifest,
            Err(error) => {
                self.remove_tree(&pending).await;
                return Err(error.into());
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
        self.sys.fs_write_async(&staging, &manifest).await?;
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
    async fn mark_used(&self, commit_dir: &Path) {
        let marker = used_marker(commit_dir);
        if let Err(error) = self.sys.fs_write_async(&marker, b"").await {
            tracing::debug!("could not mark {commit_dir:?} used: {error}");
        }
    }

    async fn prune_lockfile_pins(
        lockfile: &Lockfile,
        retained_keys: &HashSet<(Url, String)>,
    ) -> Result<(), Error> {
        lockfile
            .modify(|d| {
                match d {
                    LockfileData::V1_0_0(v1) => {
                        v1.git.retain(|uri, revs| {
                            revs.retain(|rev, _| {
                                retained_keys
                                    .contains(&(uri.clone(), rev.clone()))
                            });
                            !revs.is_empty()
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

    /// The commit currently locked for `(uri, rev)`, if any. Usable as a stable
    /// content pin.
    pub async fn locked_commit(&self, uri: &Url, rev: &str) -> Option<String> {
        self.lockfile.get_git_commit(uri, rev).await
    }

    /// Forget the locked commit for `(uri, rev)` so the next materialization
    /// re-resolves the revision. Used to advance a mutable ref (e.g. a branch)
    /// on an explicit update.
    pub async fn invalidate_git(
        &self,
        uri: &Url,
        rev: &str,
    ) -> Result<(), Error> {
        let uri = uri.clone();
        let rev = rev.to_string();
        self.lockfile
            .modify(|d| {
                match d {
                    LockfileData::V1_0_0(v1) => {
                        if let Some(revs) = v1.git.get_mut(&uri) {
                            revs.shift_remove(&rev);
                            if revs.is_empty() {
                                v1.git.shift_remove(&uri);
                            }
                        }
                    }
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// `trees/<slug>/<commit>`: the plain export of one commit.
    fn tree_dir(&self, slug: &str, commit: &str) -> PathBuf {
        self.store_root_path
            .join(TREES_SEGMENT)
            .join(slug)
            .join(commit)
    }

    /// `git/<slug>.git`: the bare mirror of one remote, shared by every commit.
    fn bare_dir(&self, slug: &str) -> PathBuf {
        self.store_root_path
            .join(GIT_KIND_SEGMENT)
            .join(format!("{slug}.git"))
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

    fn remote_lock_path(&self, slug: &str) -> PathBuf {
        self.store_root_path
            .join(LOCKS_SEGMENT)
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
    /// The remote's directory name in the store.
    pub remote: String,
    pub commit: String,
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
    /// Restrict the selection to these `(remote, commit)` pairs: the checkouts a
    /// user was shown and confirmed. A checkout added since is never deleted by
    /// a confirmed prune.
    pub only: Option<&'a [(String, String)]>,
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
            && !only.iter().any(|(remote, commit)| {
                *remote == entry.remote && *commit == entry.commit
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
    /// Every checkout in the store with its size and last use.
    pub async fn store_entries(&self) -> Result<Vec<StoreEntry>, Error> {
        self.check_layout_readonly().await?;
        let trees_root = self.store_root_path.join(TREES_SEGMENT);
        if !self.sys.fs_exists_no_err_async(&trees_root).await {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for remote_dir in self.sys.fs_read_dir_async(&trees_root).await? {
            let Some(remote) = remote_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
            else {
                continue;
            };
            let Ok(children) = self.sys.fs_read_dir_async(&remote_dir).await
            else {
                continue;
            };
            for dir in children {
                if !self.sys.fs_is_dir_no_err_async(&dir).await {
                    continue;
                }
                let Some(commit) =
                    dir.file_name().map(|n| n.to_string_lossy().into_owned())
                else {
                    continue;
                };
                let last_used = self.last_used(&dir).await;
                let size = self.dir_size(&dir).await?;
                entries.push(StoreEntry {
                    remote: remote.clone(),
                    commit,
                    dir,
                    size,
                    last_used,
                });
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
            let lock = self
                .store_root_path
                .join(LOCKS_SEGMENT)
                .join(format!("{}.lock", entry.remote));
            let _guard = LockGuard::acquire_exclusive(lock).await?;
            // Another process may have removed it while we waited.
            if self.sys.fs_exists_no_err_async(&entry.dir).await {
                self.remove_tree(&entry.dir).await;
                tracing::debug!("pruned store checkout: {:?}", entry.dir);
            }
            if let Some(parent) = entry.dir.parent() {
                // Once a remote's last tree is gone its mirror has nothing
                // left to serve, so drop it too.
                if matches!(
                    self.sys.fs_read_dir_async(parent).await.map(|d| d.len()),
                    Ok(0)
                ) {
                    let _ = self.sys.fs_remove_dir_all_async(parent).await;
                    let _ = self
                        .sys
                        .fs_remove_dir_all_async(self.bare_dir(&entry.remote))
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
    use super::*;

    fn slug(uri: &str) -> String {
        store_slug(&Url::parse(uri).unwrap())
    }

    #[test]
    fn distinct_remotes_never_share_a_store_name() {
        let all = [
            "https://host/a/b",
            "https://host/a_b",
            "https://host/a/b/c",
            "http://host/a/b",
            "https://host:8443/a/b",
            "https://other/a/b",
            "ssh://git@host/a/b",
        ];
        let mut seen = std::collections::HashSet::new();
        for uri in all {
            assert!(seen.insert(slug(uri)), "collision for {uri}");
        }
    }

    #[test]
    fn the_same_remote_keeps_one_name_regardless_of_credentials_or_dot_git() {
        let plain = slug("https://host/owner/repo");
        assert_eq!(plain, slug("https://user:token@host/owner/repo"));
        assert_eq!(plain, slug("https://host/owner/repo.git"));
        assert_eq!(plain, slug("https://host/owner/repo/"));
    }

    #[test]
    fn a_name_is_a_safe_single_path_segment() {
        let name = slug("https://host:8443/a b/c?x=1#frag");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric()
                    || matches!(c, '-' | '.' | '_')),
            "{name}"
        );
    }
}
