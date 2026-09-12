use std::path::{Path, PathBuf};

use system_traits::{FsMetadataAsync, FsReadAsync, FsWriteAsync};

use crate::{error::IgnoreError, fence};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    UpToDate,
    Missing,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanOutcome {
    Removed,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport<T> {
    pub path: PathBuf,
    pub outcome: T,
}

pub trait PatchSys:
    FsReadAsync + FsWriteAsync + FsMetadataAsync + Sync
{
}
impl<T: FsReadAsync + FsWriteAsync + FsMetadataAsync + Sync> PatchSys for T {}

async fn read_existing<S: PatchSys>(
    sys: &S,
    path: &Path,
) -> Result<Option<String>, IgnoreError> {
    if !sys.fs_exists_no_err_async(path).await {
        return Ok(None);
    }
    let content =
        sys.fs_read_to_string_async(path).await.map_err(|source| {
            IgnoreError::Io {
                action: "read",
                path: path.to_path_buf(),
                source,
            }
        })?;
    Ok(Some(content.into_owned()))
}

/// Patch the managed block into every file. Every file is read and validated
/// before any is written, so a malformed fence in one file never leaves another
/// half-patched. Only files whose content actually changes are written.
pub async fn sync_files<S: PatchSys>(
    sys: &S,
    files: &[PathBuf],
    block: &str,
) -> Result<Vec<FileReport<SyncOutcome>>, IgnoreError> {
    struct Plan {
        path: PathBuf,
        existed: bool,
        original: String,
        new_content: String,
    }

    let mut plans = Vec::with_capacity(files.len());
    for path in files {
        let existing = read_existing(sys, path).await?;
        let existed = existing.is_some();
        let original = existing.unwrap_or_default();
        let spliced = fence::splice(&original, block).map_err(|source| {
            IgnoreError::MalformedFence {
                path: path.clone(),
                source,
            }
        })?;
        plans.push(Plan {
            path: path.clone(),
            existed,
            original,
            new_content: spliced.content,
        });
    }

    let mut reports = Vec::with_capacity(plans.len());
    for plan in plans {
        let outcome = if plan.existed && plan.new_content == plan.original {
            SyncOutcome::Unchanged
        } else {
            write_file(sys, &plan.path, &plan.new_content).await?;
            if plan.existed {
                SyncOutcome::Updated
            } else {
                SyncOutcome::Created
            }
        };
        reports.push(FileReport {
            path: plan.path,
            outcome,
        });
    }
    Ok(reports)
}

/// Report whether each file is up to date without writing anything. A file is
/// `Missing` when it has no block and `Stale` when its block differs from the
/// freshly rendered one.
pub async fn check_files<S: PatchSys>(
    sys: &S,
    files: &[PathBuf],
    block: &str,
) -> Result<Vec<FileReport<CheckOutcome>>, IgnoreError> {
    let mut reports = Vec::with_capacity(files.len());
    for path in files {
        let outcome = match read_existing(sys, path).await? {
            None => CheckOutcome::Missing,
            Some(original) => {
                let spliced =
                    fence::splice(&original, block).map_err(|source| {
                        IgnoreError::MalformedFence {
                            path: path.clone(),
                            source,
                        }
                    })?;
                if !spliced.was_present {
                    CheckOutcome::Missing
                } else if spliced.content != original {
                    CheckOutcome::Stale
                } else {
                    CheckOutcome::UpToDate
                }
            }
        };
        reports.push(FileReport {
            path: path.clone(),
            outcome,
        });
    }
    Ok(reports)
}

/// Remove the managed block from every file, preserving surrounding lines.
pub async fn clean_files<S: PatchSys>(
    sys: &S,
    files: &[PathBuf],
) -> Result<Vec<FileReport<CleanOutcome>>, IgnoreError> {
    struct Plan {
        path: PathBuf,
        new_content: Option<String>,
    }

    let mut plans = Vec::with_capacity(files.len());
    for path in files {
        let new_content = match read_existing(sys, path).await? {
            None => None,
            Some(original) => fence::remove(&original).map_err(|source| {
                IgnoreError::MalformedFence {
                    path: path.clone(),
                    source,
                }
            })?,
        };
        plans.push(Plan {
            path: path.clone(),
            new_content,
        });
    }

    let mut reports = Vec::with_capacity(plans.len());
    for plan in plans {
        let outcome = match plan.new_content {
            Some(content) => {
                write_file(sys, &plan.path, &content).await?;
                CleanOutcome::Removed
            }
            None => CleanOutcome::Absent,
        };
        reports.push(FileReport {
            path: plan.path,
            outcome,
        });
    }
    Ok(reports)
}

async fn write_file<S: PatchSys>(
    sys: &S,
    path: &Path,
    content: &str,
) -> Result<(), IgnoreError> {
    sys.fs_write_async(path, content.as_bytes())
        .await
        .map_err(|source| IgnoreError::Io {
            action: "write",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(test)]
mod tests {
    use system_traits::{FsCreateDirAllAsync, impls::InMemorySys};

    use super::*;

    const BLOCK: &str = "# @@omni-managed:begin (managed by `omni ignore sync`; do not edit by hand)\n/a\n# @@omni-managed:end deadbeef";

    async fn workspace() -> InMemorySys {
        let sys = InMemorySys::default();
        sys.fs_create_dir_all_async(Path::new("/ws")).await.unwrap();
        sys
    }

    async fn read(sys: &InMemorySys, path: &str) -> Option<String> {
        if sys.fs_exists_no_err_async(Path::new(path)).await {
            Some(
                sys.fs_read_to_string_async(Path::new(path))
                    .await
                    .unwrap()
                    .into_owned(),
            )
        } else {
            None
        }
    }

    #[tokio::test]
    async fn sync_creates_a_missing_file() {
        let sys = workspace().await;
        let files = vec![PathBuf::from("/ws/.gitignore")];
        let reports = sync_files(&sys, &files, BLOCK).await.unwrap();
        assert_eq!(reports[0].outcome, SyncOutcome::Created);
        assert_eq!(
            read(&sys, "/ws/.gitignore").await.unwrap(),
            format!("{BLOCK}\n")
        );
    }

    #[tokio::test]
    async fn sync_is_idempotent() {
        let sys = workspace().await;
        let files = vec![PathBuf::from("/ws/.gitignore")];
        sync_files(&sys, &files, BLOCK).await.unwrap();
        let reports = sync_files(&sys, &files, BLOCK).await.unwrap();
        assert_eq!(reports[0].outcome, SyncOutcome::Unchanged);
    }

    #[tokio::test]
    async fn a_malformed_fence_aborts_every_write() {
        let sys = workspace().await;
        sys.fs_write_async(Path::new("/ws/good"), b"clean\n")
            .await
            .unwrap();
        let bad = format!("{BLOCK}\n{BLOCK}\n");
        sys.fs_write_async(Path::new("/ws/bad"), bad.as_bytes())
            .await
            .unwrap();

        let files = vec![PathBuf::from("/ws/good"), PathBuf::from("/ws/bad")];
        let err = sync_files(&sys, &files, BLOCK).await.unwrap_err();
        assert!(matches!(err, IgnoreError::MalformedFence { .. }));
        assert_eq!(read(&sys, "/ws/good").await.unwrap(), "clean\n");
    }

    #[tokio::test]
    async fn check_reports_missing_then_up_to_date_then_stale() {
        let sys = workspace().await;
        let files = vec![PathBuf::from("/ws/.gitignore")];

        let reports = check_files(&sys, &files, BLOCK).await.unwrap();
        assert_eq!(reports[0].outcome, CheckOutcome::Missing);

        sync_files(&sys, &files, BLOCK).await.unwrap();
        let reports = check_files(&sys, &files, BLOCK).await.unwrap();
        assert_eq!(reports[0].outcome, CheckOutcome::UpToDate);

        let updated = "# @@omni-managed:begin (managed by `omni ignore sync`; do not edit by hand)\n/a\n/b\n# @@omni-managed:end feedface";
        let reports = check_files(&sys, &files, updated).await.unwrap();
        assert_eq!(reports[0].outcome, CheckOutcome::Stale);
    }

    #[tokio::test]
    async fn clean_removes_the_block_and_reports_absent_otherwise() {
        let sys = workspace().await;
        sys.fs_write_async(
            Path::new("/ws/.gitignore"),
            format!("head\n{BLOCK}\ntail\n").as_bytes(),
        )
        .await
        .unwrap();
        sys.fs_write_async(Path::new("/ws/.ignore"), b"unmanaged\n")
            .await
            .unwrap();

        let files = vec![
            PathBuf::from("/ws/.gitignore"),
            PathBuf::from("/ws/.ignore"),
        ];
        let reports = clean_files(&sys, &files).await.unwrap();
        assert_eq!(reports[0].outcome, CleanOutcome::Removed);
        assert_eq!(reports[1].outcome, CleanOutcome::Absent);
        assert_eq!(read(&sys, "/ws/.gitignore").await.unwrap(), "head\ntail\n");
        assert_eq!(read(&sys, "/ws/.ignore").await.unwrap(), "unmanaged\n");
    }
}
