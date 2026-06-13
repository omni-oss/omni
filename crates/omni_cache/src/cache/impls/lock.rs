//! Cross-process advisory locking for the local cache directory.
//!
//! The cache directory is shared by every concurrently running `omni`
//! process. A handful of operations cannot be made safe by content-addressed
//! atomic publishing alone (see [`super::hybrid`]):
//!
//! * read-modify-write of the shared last-used-timestamps database, and
//! * pruning, which deletes cache entries that another process may be
//!   publishing into at the same moment.
//!
//! Those operations are serialized with OS advisory file locks (`flock` on
//! Unix, `LockFileEx` on Windows) taken over dedicated lock files under
//! `<cache_dir>/.locks/`. We rely on the standard library's
//! [`std::fs::File`] locking API (stable since Rust 1.89), so this needs no
//! extra dependencies.
//!
//! The lock is released when the guard is dropped, and the OS releases it
//! automatically when the owning process exits. A crashed run therefore can
//! never permanently wedge the cache.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Directory (relative to the cache dir) that holds all lock files.
pub(crate) const LOCKS_DIR: &str = ".locks";
/// Lock file guarding pruning vs. publishing.
pub(crate) const PRUNE_LOCK_FILE: &str = "prune.lock";
/// Lock file guarding the last-used-timestamps database.
pub(crate) const LAST_USED_LOCK_FILE: &str = "last-used.lock";

/// An acquired advisory lock over a single lock file.
///
/// Dropping the guard releases the lock.
#[must_use = "the lock is released as soon as the guard is dropped"]
pub(crate) struct CacheLockGuard {
    // The lock is tied to this open file handle. Keeping it alive keeps the
    // lock held; dropping it (closing the fd) releases the lock.
    file: fs::File,
}

impl CacheLockGuard {
    /// Acquire an exclusive (writer) lock, blocking until it is available.
    ///
    /// Only one holder may hold the exclusive lock at a time, and it excludes
    /// all shared holders.
    pub(crate) async fn acquire_exclusive(
        lock_path: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        Self::acquire(lock_path.into(), true).await
    }

    /// Acquire a shared (reader) lock, blocking until it is available.
    ///
    /// Any number of shared holders may hold the lock simultaneously, but a
    /// shared lock excludes the exclusive lock.
    pub(crate) async fn acquire_shared(
        lock_path: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        Self::acquire(lock_path.into(), false).await
    }

    async fn acquire(lock_path: PathBuf, exclusive: bool) -> io::Result<Self> {
        // File locking is a blocking syscall, so run it off the async runtime
        // to avoid stalling a worker thread while we wait on a peer process.
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = lock_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                // We only ever lock this file, never read/write its contents,
                // so preserve whatever is there rather than truncating.
                .truncate(false)
                .open(&lock_path)?;

            if exclusive {
                file.lock()?;
            } else {
                file.lock_shared()?;
            }

            Ok(Self { file })
        })
        .await
        .map_err(io::Error::other)?
    }
}

impl Drop for CacheLockGuard {
    fn drop(&mut self) {
        // Best effort: closing the handle would release the lock anyway, but
        // unlocking explicitly makes the intent clear and releases promptly.
        let _ = self.file.unlock();
    }
}

/// Builds the path of a lock file inside the cache directory's lock folder.
pub(crate) fn lock_file_path(cache_dir: &Path, name: &str) -> PathBuf {
    cache_dir.join(LOCKS_DIR).join(name)
}
