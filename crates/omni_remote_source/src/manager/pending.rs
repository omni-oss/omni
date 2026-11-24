//! Ownership and reclamation of `.pending` paths.
//!
//! A publisher exports into `<store>/.pending/<name>` (a directory) or stages a
//! manifest there (a file). Each path has a unique sibling lock file
//! `<name>.lock` that the publisher holds exclusively for as long as the path
//! may exist. The kernel releases the lock when the process dies, so a cleaner
//! that can take it knows the owner is gone, whatever the pid in the name says.
//!
//! Unlike lease files, these lock files are unique and single-use, so they may
//! be unlinked together with the path (a lease file must never be, because
//! unlinking lets two openers lock different files at one path). Two parties
//! ever open one: the owner, and a cleaner that probes it without creating it.
//! The owner can lose a race with a cleaner between creating and locking the
//! file, so it re-checks that the file still exists once locked (the grace
//! period makes that race very unlikely). The lock file is deleted only after
//! its path is confirmed gone, because the cleaner finds paths through their
//! lock files: deleting the lock of a path that could not be removed would
//! orphan it forever. A cancelled [`PendingPath::create`] can leave an orphan
//! lock file with no path; the cleaner removes it after the grace period
//! without reporting it.
//!
//! Windows: the owner unlinks its lock file while holding the handle, which
//! relies on std opening files with `FILE_SHARE_DELETE`. If a delete is still
//! pending or fails, the entry is skipped and retried by the next cleanup.
//! Entries left by older omni versions have no lock file and are never
//! reclaimed.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use omni_utils::lock::LockGuard;

use crate::error::Error;

/// Entries younger than this are never reclaimed, so a cleaner cannot race a
/// publisher that is still setting up.
pub(crate) const PENDING_GRACE: Duration = Duration::from_secs(60);

const LOCK_SUFFIX: &str = ".lock";

/// Process-wide counter making pending names unique even when a single process
/// publishes several checkouts at once.
static PENDING_COUNTER: AtomicU64 = AtomicU64::new(0);

fn pending_root(store_root: &Path) -> PathBuf {
    store_root.join(omni_constants::PENDING_SEGMENT)
}

fn lock_path_for(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// Remove `path` (file or directory). `true` means it is gone; a missing path
/// counts as gone.
fn remove_path(path: &Path) -> bool {
    let result = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) => Err(error),
    };
    match result {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            tracing::debug!("could not remove pending path {path:?}: {error}");
            false
        }
    }
}

/// Remove the path, then its lock file only if the path is gone.
fn remove_entry(path: &Path, lock_path: &Path) -> bool {
    if !remove_path(path) {
        return false;
    }
    if let Err(error) = fs::remove_file(lock_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::debug!("could not remove pending lock {lock_path:?}: {error}");
    }
    true
}

/// A pending path this process owns. Dropping it removes the path and its lock
/// file, synchronously and best effort, on every exit: error, cancellation or
/// panic. If the process is killed instead, the path stays behind for
/// [`reclaim`].
///
/// The caller creates the path (directory or file) at [`path`](Self::path). A
/// path that was renamed away is simply absent at drop.
#[must_use = "dropping a PendingPath removes it"]
pub(crate) struct PendingPath {
    path: PathBuf,
    lock_path: PathBuf,
    _guard: Option<LockGuard>,
    cleanup: bool,
}

impl PendingPath {
    /// Reserve a unique pending name and lock it. Nothing exists at
    /// [`path`](Self::path) yet.
    pub(crate) async fn create(store_root: &Path) -> Result<Self, Error> {
        loop {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = PENDING_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = pending_root(store_root)
                .join(format!("{}-{nanos}-{seq}", std::process::id()));
            let lock_path = lock_path_for(&path);
            let Some(guard) = lock_new(&lock_path, || {}).await? else {
                continue;
            };
            return Ok(Self {
                path,
                lock_path,
                _guard: Some(guard),
                cleanup: true,
            });
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

/// Create and lock `lock_path`. `None` means the file vanished before the lock
/// was taken (a cleaner removed it), so the lock protects nothing. `hook` runs
/// between locking and the check, for tests to interleave a cleaner.
async fn lock_new(
    lock_path: &Path,
    hook: impl FnOnce(),
) -> Result<Option<LockGuard>, Error> {
    let guard = LockGuard::acquire_exclusive(lock_path).await?;
    hook();
    if !tokio::fs::try_exists(lock_path).await? {
        return Ok(None);
    }
    Ok(Some(guard))
}

impl Drop for PendingPath {
    fn drop(&mut self) {
        if !self.cleanup {
            return;
        }
        // Still holding the lock, so no cleaner is mid-way through this entry.
        // If the path cannot be removed its lock file stays, so a later cleanup
        // can find and retry it.
        remove_entry(&self.path, &self.lock_path);
    }
}

/// Reclaim pending paths whose owner is gone: those older than `grace` whose
/// lock this call can take exclusively. Returns the reclaimed paths (the ones
/// that would be, for a `dry_run`). A live owner holds its lock and is skipped.
/// Entries without a lock file are not ours (older omni versions) and are left.
/// An entry whose path cannot be removed keeps its lock file, is not reported,
/// and is retried on the next run.
pub(crate) async fn reclaim(
    store_root: &Path,
    grace: Duration,
    dry_run: bool,
) -> Result<Vec<PathBuf>, Error> {
    let root = pending_root(store_root);
    let mut names = match tokio::fs::read_dir(&root).await {
        Ok(names) => names,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.into()),
    };
    let mut locks = Vec::new();
    while let Some(entry) = names.next_entry().await? {
        let file_name = entry.file_name();
        if file_name.to_string_lossy().ends_with(LOCK_SUFFIX) {
            locks.push(entry.path());
        }
    }
    locks.sort();

    let now = SystemTime::now();
    let mut reclaimed = Vec::new();
    for lock_path in locks {
        let Ok(meta) = tokio::fs::metadata(&lock_path).await else {
            continue;
        };
        let age = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .unwrap_or_default();
        if age < grace {
            continue;
        }
        // Never create: the owner may have just removed the file.
        let guard =
            match LockGuard::try_acquire_exclusive_existing(&lock_path).await {
                Ok(Some(guard)) => guard,
                Ok(None) => continue,
                Err(error) => {
                    tracing::debug!("pending lock {lock_path:?}: {error}");
                    continue;
                }
            };
        let lock_name = lock_path.to_string_lossy();
        let path =
            PathBuf::from(&lock_name[..lock_name.len() - LOCK_SUFFIX.len()]);
        let exists = tokio::fs::symlink_metadata(&path).await.is_ok();
        if dry_run {
            if exists {
                reclaimed.push(path);
            }
        } else {
            let (removing, removing_lock) = (path.clone(), lock_path.clone());
            let removed = tokio::task::spawn_blocking(move || {
                remove_entry(&removing, &removing_lock)
            })
            .await
            .unwrap_or(false);
            // An orphan lock (no path) is cleaned but was never a leftover.
            if removed && exists {
                reclaimed.push(path);
            }
        }
        drop(guard);
    }
    Ok(reclaimed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_GRACE: Duration = Duration::ZERO;

    async fn dir_pending(root: &Path) -> PendingPath {
        let pending = PendingPath::create(root).await.unwrap();
        fs::create_dir_all(pending.path().join("sub")).unwrap();
        fs::write(pending.path().join("sub/f"), "x").unwrap();
        pending
    }

    /// Simulates a killed owner: the lock is released, nothing is cleaned up.
    fn crash(mut pending: PendingPath) -> (PathBuf, PathBuf) {
        pending.cleanup = false;
        pending._guard = None;
        (pending.path.clone(), pending.lock_path.clone())
    }

    #[tokio::test]
    async fn a_crashed_owners_path_is_reclaimed() {
        let store = tempfile::tempdir().unwrap();
        let (path, lock) = crash(dir_pending(store.path()).await);
        assert!(path.exists() && lock.exists());

        let got = reclaim(store.path(), NO_GRACE, false).await.unwrap();
        assert_eq!(got, vec![path.clone()]);
        assert!(!path.exists() && !lock.exists());
    }

    #[tokio::test]
    async fn a_live_owners_path_is_left_alone() {
        let store = tempfile::tempdir().unwrap();
        let pending = dir_pending(store.path()).await;

        let got = reclaim(store.path(), NO_GRACE, false).await.unwrap();
        assert!(got.is_empty());
        assert!(pending.path().join("sub/f").exists());
    }

    #[tokio::test]
    async fn a_young_entry_is_left_alone_even_when_unlocked() {
        let store = tempfile::tempdir().unwrap();
        let (path, _) = crash(dir_pending(store.path()).await);

        let got = reclaim(store.path(), PENDING_GRACE, false).await.unwrap();
        assert!(got.is_empty());
        assert!(path.exists());
    }

    #[tokio::test]
    async fn a_staging_file_is_reclaimed() {
        let store = tempfile::tempdir().unwrap();
        let pending = PendingPath::create(store.path()).await.unwrap();
        fs::write(pending.path(), "manifest").unwrap();
        let (path, lock) = crash(pending);

        let got = reclaim(store.path(), NO_GRACE, false).await.unwrap();
        assert_eq!(got, vec![path.clone()]);
        assert!(!path.exists() && !lock.exists());
    }

    #[tokio::test]
    async fn a_dry_run_reports_without_deleting() {
        let store = tempfile::tempdir().unwrap();
        let (path, lock) = crash(dir_pending(store.path()).await);

        let got = reclaim(store.path(), NO_GRACE, true).await.unwrap();
        assert_eq!(got, vec![path.clone()]);
        assert!(path.exists() && lock.exists());
    }

    #[tokio::test]
    async fn an_entry_without_a_lock_file_is_not_touched() {
        let store = tempfile::tempdir().unwrap();
        let legacy = store.path().join(".pending/123-1-0");
        fs::create_dir_all(&legacy).unwrap();

        let got = reclaim(store.path(), NO_GRACE, false).await.unwrap();
        assert!(got.is_empty());
        assert!(legacy.exists());
    }

    #[tokio::test]
    async fn dropping_the_guard_removes_path_and_lock() {
        let store = tempfile::tempdir().unwrap();
        let pending = dir_pending(store.path()).await;
        let (path, lock) = (pending.path.clone(), pending.lock_path.clone());
        drop(pending);
        assert!(!path.exists() && !lock.exists());
    }

    #[tokio::test]
    async fn a_lock_removed_before_it_is_locked_is_detected() {
        let store = tempfile::tempdir().unwrap();
        let lock = store.path().join("x.lock");
        let hook_lock = lock.clone();
        let got = lock_new(&lock, move || fs::remove_file(&hook_lock).unwrap())
            .await
            .unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn an_orphan_lock_is_removed_but_not_reported() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path().join(".pending");
        fs::create_dir_all(&root).unwrap();
        let lock = root.join("1-1-0.lock");
        let file = fs::File::create(&lock).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();

        let dry = reclaim(store.path(), PENDING_GRACE, true).await.unwrap();
        assert!(dry.is_empty());
        assert!(lock.exists());
        let real = reclaim(store.path(), PENDING_GRACE, false).await.unwrap();
        assert!(real.is_empty());
        assert!(!lock.exists());
    }

    #[tokio::test]
    async fn the_cleaner_does_not_recreate_a_removed_lock() {
        let store = tempfile::tempdir().unwrap();
        let lock = store.path().join("gone.lock");
        let got = LockGuard::try_acquire_exclusive_existing(&lock)
            .await
            .unwrap();
        assert!(got.is_none());
        assert!(!lock.exists());
    }

    /// A directory that cannot be deleted: a read-only subdirectory with a
    /// file in it. `None` when permissions are not enforced (running as root).
    #[cfg(unix)]
    fn undeletable(pending: &Path) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        let sub = pending.join("ro");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("f"), "x").unwrap();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).unwrap();
        if fs::write(sub.join("probe"), "x").is_ok() {
            fs::set_permissions(&sub, fs::Permissions::from_mode(0o755))
                .unwrap();
            return None;
        }
        Some(sub)
    }

    #[cfg(unix)]
    fn restore(sub: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(sub, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_removal_keeps_the_lock_and_is_not_reported() {
        let store = tempfile::tempdir().unwrap();
        let (path, lock) = crash(dir_pending(store.path()).await);
        let Some(sub) = undeletable(&path) else {
            return;
        };

        let got = reclaim(store.path(), NO_GRACE, false).await.unwrap();
        assert!(got.is_empty());
        assert!(path.exists() && lock.exists());

        restore(&sub);
        let got = reclaim(store.path(), NO_GRACE, false).await.unwrap();
        assert_eq!(got, vec![path.clone()]);
        assert!(!path.exists() && !lock.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failed_drop_removal_keeps_the_lock_for_the_cleaner() {
        let store = tempfile::tempdir().unwrap();
        let pending = dir_pending(store.path()).await;
        let (path, lock) = (pending.path.clone(), pending.lock_path.clone());
        let Some(sub) = undeletable(&path) else {
            return;
        };
        drop(pending);
        assert!(path.exists() && lock.exists());

        restore(&sub);
        let got = reclaim(store.path(), NO_GRACE, false).await.unwrap();
        assert_eq!(got, vec![path.clone()]);
        assert!(!path.exists() && !lock.exists());
    }

    #[tokio::test]
    async fn a_missing_pending_root_is_fine() {
        let store = tempfile::tempdir().unwrap();
        assert!(
            reclaim(store.path(), NO_GRACE, false)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
