//! Cross-process advisory file locking.
//!
//! Provides an OS-level advisory lock (`flock` on Unix, `LockFileEx` on
//! Windows) that serializes access to shared resources across processes.
//! The lock is released when the guard is dropped, and the OS releases it
//! automatically when the owning process exits — a crashed process can never
//! permanently wedge a locked resource.

use std::{fs, io, path::PathBuf};

/// An acquired advisory lock over a single lock file.
///
/// Dropping the guard releases the lock.
#[must_use = "the lock is released as soon as the guard is dropped"]
pub struct LockGuard {
    // Keeping the handle alive keeps the lock held; closing it releases it.
    file: fs::File,
}

impl LockGuard {
    /// Acquire an exclusive (writer) lock on `lock_path`, blocking until
    /// it is available. Only one holder may hold the exclusive lock at a
    /// time, and it excludes all shared holders.
    pub async fn acquire_exclusive(
        lock_path: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        Self::acquire(lock_path.into(), true).await
    }

    /// Acquire a shared (reader) lock on `lock_path`, blocking until it is
    /// available. Any number of shared holders may hold the lock
    /// simultaneously, but a shared lock excludes the exclusive lock.
    pub async fn acquire_shared(
        lock_path: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        Self::acquire(lock_path.into(), false).await
    }

    async fn acquire(lock_path: PathBuf, exclusive: bool) -> io::Result<Self> {
        // File locking is a blocking syscall; run it off the async runtime to
        // avoid stalling a worker thread while waiting on a peer process.
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = lock_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                // We only ever lock this file, never read/write its contents.
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

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
