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
//! Those operations are serialized with OS advisory file locks taken over
//! dedicated lock files under `<cache_dir>/.locks/`. See [`omni_utils::lock`]
//! for the underlying implementation.

use std::path::{Path, PathBuf};

pub(crate) use omni_utils::lock::LockGuard as CacheLockGuard;

/// Directory (relative to the cache dir) that holds all lock files.
pub(crate) const LOCKS_DIR: &str = ".locks";
/// Lock file guarding pruning vs. publishing.
pub(crate) const PRUNE_LOCK_FILE: &str = "prune.lock";
/// Lock file guarding the last-used-timestamps database.
pub(crate) const LAST_USED_LOCK_FILE: &str = "last-used.lock";

/// Builds the path of a lock file inside the cache directory's lock folder.
pub(crate) fn lock_file_path(cache_dir: &Path, name: &str) -> PathBuf {
    cache_dir.join(LOCKS_DIR).join(name)
}
