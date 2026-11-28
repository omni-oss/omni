use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use system_traits::{
    FsMetadataAsync as _, FsMetadataValue as _, FsReadDirAsync as _,
    FsRemoveDirAllAsync as _, FsRenameAsync as _, impls::RealSys,
};

/// Prefix for in-progress staging directories. A staging directory is
/// populated in full and then atomically renamed into its final location, so
/// readers never observe a half-written entry.
pub const STAGING_PREFIX: &str = ".staging-";

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Builds a unique staging directory path that is a sibling of `final_dir`
/// (and therefore on the same filesystem, so the publishing rename is atomic).
pub fn staging_dir_for(final_dir: &Path) -> PathBuf {
    let parent = final_dir.parent().unwrap_or(final_dir);
    let name = final_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);

    parent.join(format!("{STAGING_PREFIX}{name}-{pid}-{nanos}-{seq}"))
}

/// Existence check that does not follow symlinks (a dangling symlink exists),
/// matching the semantics the task cache has always used.
async fn exists_no_follow(sys: &RealSys, path: &Path) -> io::Result<bool> {
    match sys.fs_symlink_metadata_async(path).await {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Atomically publishes a fully-populated staging directory to `final_dir`.
///
/// `sentinel_path` is the file inside `final_dir` whose presence marks an
/// entry complete. When `replace` is `false` a complete entry that already
/// exists is assumed equivalent, so losing a publish race is a success. When
/// `replace` is `true` an existing entry is cleared and replaced. A destination
/// without the sentinel is a partial leftover and is always cleared.
///
/// Returns once the destination holds a complete entry. The staging directory
/// is removed on every failure path.
pub async fn publish_staged_dir(
    sys: &RealSys,
    staging_dir: &Path,
    final_dir: &Path,
    sentinel_path: &Path,
    replace: bool,
) -> io::Result<()> {
    const MAX_ATTEMPTS: usize = 5;

    for _ in 0..MAX_ATTEMPTS {
        match sys.fs_rename_async(staging_dir, final_dir).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                let complete = if replace {
                    Ok(false)
                } else {
                    exists_no_follow(sys, sentinel_path).await
                };
                match complete {
                    Ok(true) => {
                        let _ = sys.fs_remove_dir_all_async(staging_dir).await;
                        return Ok(());
                    }
                    Ok(false) => {}
                    Err(check) => {
                        let _ = sys.fs_remove_dir_all_async(staging_dir).await;
                        return Err(check);
                    }
                }

                match exists_no_follow(sys, final_dir).await {
                    Ok(true) => {
                        let _ = sys.fs_remove_dir_all_async(final_dir).await;
                        continue;
                    }
                    Ok(false) => {}
                    Err(check) => {
                        let _ = sys.fs_remove_dir_all_async(staging_dir).await;
                        return Err(check);
                    }
                }

                let _ = sys.fs_remove_dir_all_async(staging_dir).await;
                return Err(e);
            }
        }
    }

    let _ = sys.fs_remove_dir_all_async(staging_dir).await;
    Err(io::Error::other(
        "failed to publish cache entry after multiple attempts",
    ))
}

/// Removes staging directories directly under `parent` that are at least
/// `max_age` old. Fresh ones (a live peer's) are left alone. Best-effort:
/// failures are logged and skipped.
pub async fn reclaim_stale_staging_dirs(
    sys: &RealSys,
    parent: &Path,
    max_age: Duration,
) {
    let now = SystemTime::now();
    let entries = match sys.fs_read_dir_async(parent).await {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries {
        if !entry.file_name().is_some_and(|name| {
            name.to_string_lossy().starts_with(STAGING_PREFIX)
        }) {
            continue;
        }

        // Staleness is the staging dir's own mtime: `DirEntry::metadata` never
        // followed a symlink, so neither does this.
        let stale = sys
            .fs_symlink_metadata_async(&entry)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);

        if stale && let Err(e) = sys.fs_remove_dir_all_async(&entry).await {
            tracing::debug!(
                "failed to reclaim stale staging dir {}: {e}",
                entry.display()
            );
        }
    }
}
