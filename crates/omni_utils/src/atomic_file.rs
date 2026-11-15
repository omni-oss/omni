//! Single-file atomic publication through the caller's filesystem abstraction.

use std::{
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use system_traits::{
    FsFileSyncAll, FsOpen, FsRemoveFile, FsRename, OpenOptions,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Write, flush and sync a new file beside `path`, then rename it over `path`.
///
/// The parent must exist. Callers serialize read-modify-write externally and run
/// this blocking operation off async workers. Concurrent readers see either the
/// old or the new complete file on filesystems supporting replacement rename.
/// No directory sync is performed: this is not a power-loss durability promise.
/// Staging files never end in `.json`, so source reconciliation ignores them.
pub fn write(
    sys: &(impl FsOpen + FsRename + FsRemoveFile),
    path: &Path,
    bytes: &[u8],
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut options = OpenOptions::new();
    options.write = true;
    options.create = true;
    options.create_new = true;
    // Exclusive creation prevents following a pre-existing staging symlink.
    // Bounded retries handle leftovers from an earlier process with our PID.
    for _ in 0..32 {
        let staging = parent.join(format!(
            ".omni-atomic-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match sys.fs_open(&staging, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                continue;
            }
            Err(error) => return Err(error),
        };
        let result = (|| {
            file.write_all(bytes)?;
            file.flush()?;
            file.fs_file_sync_all()
        })();
        // Close before replacement and cleanup (especially on Windows).
        drop(file);
        let result = result.and_then(|()| sys.fs_rename(&staging, path));
        if let Err(error) = result {
            if let Err(cleanup) = sys.fs_remove_file(&staging)
                && cleanup.kind() != io::ErrorKind::NotFound
            {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; could not remove staging file {}: {cleanup}",
                        staging.display()
                    ),
                ));
            }
            return Err(error);
        }
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique atomic-publication staging file",
    ))
}
