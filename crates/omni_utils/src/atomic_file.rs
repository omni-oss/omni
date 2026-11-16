//! Staged file publication through the caller's filesystem abstraction.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
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
    write_batch(sys, &[(path, bytes)])
}

/// Stage every file before publishing any of them.
///
/// Staging failure leaves all destinations untouched. Once renaming starts,
/// failure can leave a prefix committed: each destination is old-complete or
/// new-complete, but the batch is not a cross-file transaction. Remaining staging
/// files are cleaned up, and retrying a complete batch converges.
/// The same external locking, filesystem and durability requirements as
/// [`write`] apply. Destination paths must be distinct.
pub fn write_batch(
    sys: &(impl FsOpen + FsRename + FsRemoveFile),
    files: &[(&Path, &[u8])],
) -> io::Result<()> {
    let mut staged = Vec::with_capacity(files.len());
    for &(path, bytes) in files {
        match stage(sys, path, bytes) {
            Ok(temp) => staged.push((temp, path)),
            Err(error) => return Err(cleanup(sys, &staged, error)),
        }
    }
    for (temp, path) in &staged {
        if let Err(error) = sys.fs_rename(temp, path) {
            return Err(cleanup(sys, &staged, error));
        }
    }
    Ok(())
}

fn cleanup(
    sys: &impl FsRemoveFile,
    staged: &[(PathBuf, &Path)],
    mut error: io::Error,
) -> io::Error {
    for (temp, _) in staged {
        error = cleanup_one(sys, temp, error);
    }
    error
}

fn cleanup_one(
    sys: &impl FsRemoveFile,
    temp: &Path,
    error: io::Error,
) -> io::Error {
    match sys.fs_remove_file(temp) {
        Err(cleanup) if cleanup.kind() != io::ErrorKind::NotFound => {
            io::Error::new(
                error.kind(),
                format!(
                    "{error}; could not remove staging file {}: {cleanup}",
                    temp.display()
                ),
            )
        }
        _ => error,
    }
}

fn stage(
    sys: &(impl FsOpen + FsRemoveFile),
    path: &Path,
    bytes: &[u8],
) -> io::Result<PathBuf> {
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
        if let Err(error) = result {
            return Err(cleanup_one(sys, &staging, error));
        }
        return Ok(staging);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not create a unique atomic-publication staging file",
    ))
}
