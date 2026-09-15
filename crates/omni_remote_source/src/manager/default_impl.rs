use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use omni_git_utils::CloneInfo;
use omni_lockfile::{Lockfile, data::LockfileData};
use omni_utils::lock::LockGuard;
use tokio::task::JoinSet;
use url::Url;

use crate::{
    error::Error,
    manager::config::RemoteSourceConfig,
    refs::{GitRef, RefSetData, RefSetDataV1_0_0},
    source::{MaterializedSource, RemoteSource, RemoteSourceRef},
    sys::RemoteSourceSys,
};

const GIT_KIND_SEGMENT: &str = "git";

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

    /// Reconcile the shared store against the union of every persisted
    /// reference set: delete any store checkout no subsystem references
    /// (including orphans left by an advanced mutable ref), prune unreferenced
    /// lockfile pins, and write the pruned lockfile. Runs under the exclusive
    /// advisory lock so no concurrent write races the reconcile.
    pub async fn retain(&self) -> Result<usize, Error> {
        let _guard =
            LockGuard::acquire_exclusive(self.advisory_lock_path()).await?;

        let mut retained_dirs: HashSet<PathBuf> = HashSet::new();
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
                            for (rev, git_ref) in revs {
                                retained_dirs.insert(
                                    self.git_commit_dir(&uri, &git_ref.commit)?,
                                );
                                retained_keys.insert((uri.clone(), rev));
                            }
                        }
                    }
                }
            }
        }

        let removed = self.prune_store(&retained_dirs).await?;
        self.prune_lockfile_pins(&retained_keys).await?;
        self.ensure_lockfile_parent().await?;
        self.lockfile.save(&self.sys).await?;

        Ok(removed)
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
        if let Some(commit) = self.lockfile.get_git_commit(uri, rev).await {
            let dir = self.git_commit_dir(uri, &commit)?;
            if self.sys.fs_exists_no_err_async(&dir).await {
                return Ok((dir, commit));
            }

            let published = self.publish_git(uri, &commit).await?;
            return Ok((self.git_commit_dir(uri, &published)?, published));
        }

        let commit = self.publish_git(uri, rev).await?;
        self.lockfile.lock_git_commit(uri, rev, &commit).await?;
        Ok((self.git_commit_dir(uri, &commit)?, commit))
    }

    async fn publish_git(&self, uri: &Url, rev: &str) -> Result<String, Error> {
        let pending = self.new_pending_dir();
        let clone = self.clone_repo_inner(&pending, uri, rev).await?;
        let commit = clone.commit;
        let dest = self.git_commit_dir(uri, &commit)?;

        if self.sys.fs_exists_no_err_async(&dest).await {
            let _ = self.sys.fs_remove_dir_all_async(&pending).await;
            return Ok(commit);
        }

        if let Some(parent) = dest.parent() {
            self.sys.fs_create_dir_all_async(parent).await?;
        }

        match self.sys.fs_rename_async(&pending, &dest).await {
            Ok(()) => Ok(commit),
            Err(err) => {
                let _ = self.sys.fs_remove_dir_all_async(&pending).await;
                if self.sys.fs_exists_no_err_async(&dest).await {
                    Ok(commit)
                } else {
                    Err(err.into())
                }
            }
        }
    }

    async fn prune_store(
        &self,
        retained_dirs: &HashSet<PathBuf>,
    ) -> Result<usize, Error> {
        let git_root = self.store_root_path.join(GIT_KIND_SEGMENT);
        if !self.sys.fs_exists_no_err_async(&git_root).await {
            return Ok(0);
        }

        let mut removed = 0;
        for slug_dir in self.sys.fs_read_dir_async(&git_root).await? {
            let commit_dirs = match self.sys.fs_read_dir_async(&slug_dir).await
            {
                Ok(dirs) => dirs,
                Err(_) => continue,
            };

            for commit_dir in commit_dirs {
                if !retained_dirs.contains(&commit_dir) {
                    self.sys.fs_remove_dir_all_async(&commit_dir).await?;
                    removed += 1;
                    log::debug!(
                        "removed unreferenced store checkout: {commit_dir:?}"
                    );
                }
            }
        }

        Ok(removed)
    }

    async fn prune_lockfile_pins(
        &self,
        retained_keys: &HashSet<(Url, String)>,
    ) -> Result<(), Error> {
        self.lockfile
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
    pub async fn pull_git_repo(
        &self,
        uri: &Url,
        rev: &str,
    ) -> Result<PathBuf, Error> {
        let commit = self.lockfile.get_git_commit(uri, rev).await;
        let dest = self.git_dest_dir(uri, Some(rev))?;

        match commit {
            Some(commit) => {
                if !self.sys.fs_exists_no_err_async(&dest).await {
                    log::trace!("created dir: {dest:?}");
                    self.sys.fs_create_dir_all_async(&dest).await?;
                    self.clone_repo_inner(&dest, uri, &commit).await?;
                }
            }
            None => {
                if self.sys.fs_exists_no_err_async(&dest).await {
                    log::trace!("removing dir: {dest:?}");
                    self.sys.fs_remove_dir_all_async(&dest).await?;
                }
                log::trace!("created dir: {dest:?}");
                self.sys.fs_create_dir_all_async(&dest).await?;

                let clone = self
                    .clone_repo_inner(
                        &dest,
                        uri,
                        commit.as_deref().unwrap_or(rev),
                    )
                    .await?;

                self.lockfile
                    .lock_git_commit(uri, rev, &clone.commit)
                    .await?;
            }
        }

        Ok(dest)
    }

    pub async fn retain_git_sources(
        &self,
        git_sources: &[(&Url, &str)],
    ) -> Result<(), Error> {
        let mut source_map = HashMap::<&Url, HashSet<&str>>::new();

        for (url, rev) in git_sources {
            if let Some(revs) = source_map.get_mut(url) {
                revs.insert(*rev);
            } else {
                let set = HashSet::from_iter([*rev]);

                source_map.insert(*url, set);
            }
        }

        let mut rm_dirs: Vec<(Url, Option<String>)> = vec![];

        self.lockfile
            .modify(|d| {
                match d {
                    LockfileData::V1_0_0(v1) => {
                        v1.git.retain(|k, v| {
                            if !source_map.contains_key(k) {
                                rm_dirs.push((k.clone(), None));
                                return false;
                            }

                            let revs =
                                source_map.get(k).expect("should have value");

                            v.retain(|x, _| {
                                let should_retain = revs.contains(x.as_str());

                                if !should_retain {
                                    rm_dirs.push((k.clone(), Some(x.clone())));
                                }

                                should_retain
                            });

                            if v.is_empty() {
                                rm_dirs.push((k.clone(), None));
                                return false;
                            }

                            true
                        });
                    }
                }

                Ok(())
            })
            .await?;

        let mut rm_tasks = JoinSet::new();
        for (uri, rev) in rm_dirs {
            let dest = self.git_dest_dir(&uri, rev.as_deref())?;
            let sys = self.sys.clone();

            rm_tasks.spawn(async move {
                if sys.fs_exists_async(&dest).await? {
                    sys.fs_remove_dir_all_async(&dest).await?;
                    log::debug!("removed stale git source directory: {dest:?}");
                }

                Ok::<_, Error>(())
            });
        }

        for t in rm_tasks.join_all().await {
            t?;
        }

        Ok(())
    }

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

    async fn clone_repo_inner(
        &self,
        dest: &Path,
        uri: &Url,
        commit: &str,
    ) -> Result<CloneInfo, Error> {
        let clone = omni_git_utils::clone_repo(
            &self.sys,
            uri.as_str(),
            Some(commit),
            dest,
        )
        .await?;
        log::trace!("cloned git repo uri: {}, rev: {}", uri, commit,);
        Ok(clone)
    }

    fn git_dest_dir(
        &self,
        uri: &Url,
        rev: Option<&str>,
    ) -> Result<PathBuf, omni_git_utils::Error> {
        let slug = omni_git_utils::url_to_safe_dir_name(uri.as_str())?;
        let path = self.store_root_path.join("git").join(slug);

        Ok(if let Some(rev) = rev {
            path.join(rev)
        } else {
            path
        })
    }

    fn git_commit_dir(
        &self,
        uri: &Url,
        commit: &str,
    ) -> Result<PathBuf, omni_git_utils::Error> {
        let slug = omni_git_utils::url_to_safe_dir_name(uri.as_str())?;
        Ok(self
            .store_root_path
            .join(GIT_KIND_SEGMENT)
            .join(slug)
            .join(commit))
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
