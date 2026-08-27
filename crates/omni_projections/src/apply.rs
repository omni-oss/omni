use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use omni_projection_configurations::{CollisionPolicy, LinkKind};
use system_traits::{
    FileType, FsCopyAsync, FsCreateDirAllAsync, FsCreateJunctionAsync,
    FsHardLinkAsync, FsMetadataAsync, FsMetadataValue, FsReadDirAsync,
    FsRemoveDirAllAsync, FsRemoveFileAsync, FsRenameAsync, FsSymlinkDirAsync,
    FsSymlinkFileAsync,
};

use crate::error::{ProjectionError, Result};
use crate::routing::LinkPair;

/// The link kind that was actually materialized (after `auto` fallback).
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedKind {
    Symlink,
    Junction,
    Hardlink,
    Copy,
}

/// The result of applying one `LinkPair`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    Linked {
        kind: ResolvedKind,
        backup: Option<PathBuf>,
    },
    Skipped,
}

pub struct ApplyOptions {
    pub link: LinkKind,
    pub collision: CollisionPolicy,
}

/// A destination omni already owns, per the ledger. Used for the idempotent
/// "up-to-date" exception: an unchanged pin means the link is left untouched;
/// a changed pin means omni re-points it (regardless of collision policy).
pub struct PriorLink<'a> {
    pub source_abs: &'a Path,
    pub pin_unchanged: bool,
}

/// The full trait surface the applier needs from a system.
pub trait ApplierSys:
    FsSymlinkFileAsync
    + FsSymlinkDirAsync
    + FsCreateJunctionAsync
    + FsHardLinkAsync
    + FsCopyAsync
    + FsRenameAsync
    + FsRemoveFileAsync
    + FsRemoveDirAllAsync
    + FsCreateDirAllAsync
    + FsMetadataAsync
    + FsReadDirAsync
    + Sync
{
}

impl<T> ApplierSys for T where
    T: FsSymlinkFileAsync
        + FsSymlinkDirAsync
        + FsCreateJunctionAsync
        + FsHardLinkAsync
        + FsCopyAsync
        + FsRenameAsync
        + FsRemoveFileAsync
        + FsRemoveDirAllAsync
        + FsCreateDirAllAsync
        + FsMetadataAsync
        + FsReadDirAsync
        + Sync
{
}

/// Materialize one link, honoring the ownership exception and collision policy.
pub async fn apply_link<S: ApplierSys>(
    sys: &S,
    pair: &LinkPair,
    opts: &ApplyOptions,
    prior: Option<&PriorLink<'_>>,
) -> Result<ApplyOutcome> {
    if let Some(parent) = pair.dest_abs.parent() {
        sys.fs_create_dir_all_async(parent).await?;
    }

    let dest_exists = symlink_meta(sys, &pair.dest_abs).await?.is_some();

    // Ownership exception: omni already created this dest for this source.
    if let Some(prior) = prior {
        if prior.source_abs == pair.source_abs {
            if prior.pin_unchanged {
                if dest_exists {
                    return Ok(ApplyOutcome::Skipped);
                }
            } else if dest_exists {
                // Pin changed: re-point the link omni owns, no backup.
                remove_existing(sys, &pair.dest_abs).await?;
                let kind = materialize(sys, pair, opts.link).await?;
                return Ok(ApplyOutcome::Linked { kind, backup: None });
            }
        }
    }

    let mut backup = None;
    if dest_exists {
        match opts.collision {
            CollisionPolicy::Skip => return Ok(ApplyOutcome::Skipped),
            CollisionPolicy::Error => {
                return Err(ProjectionError::custom(format!(
                    "destination already exists: {}",
                    pair.dest_abs.display()
                )));
            }
            CollisionPolicy::Overwrite => {
                remove_existing(sys, &pair.dest_abs).await?;
            }
            CollisionPolicy::Backup => {
                let path = backup_path(&pair.dest_abs);
                sys.fs_rename_async(&pair.dest_abs, &path).await?;
                backup = Some(path);
            }
        }
    }

    let kind = materialize(sys, pair, opts.link).await?;
    Ok(ApplyOutcome::Linked { kind, backup })
}

async fn materialize<S: ApplierSys>(
    sys: &S,
    pair: &LinkPair,
    link: LinkKind,
) -> Result<ResolvedKind> {
    let src_is_dir = sys.fs_is_dir_no_err_async(&pair.source_abs).await;
    let rel = relative_target(&pair.dest_abs, &pair.source_abs);

    match link {
        LinkKind::Auto => {
            if try_symlink(sys, &rel, &pair.dest_abs, src_is_dir)
                .await
                .is_ok()
            {
                return Ok(ResolvedKind::Symlink);
            }
            if src_is_dir {
                if sys
                    .fs_create_junction_async(&pair.source_abs, &pair.dest_abs)
                    .await
                    .is_ok()
                {
                    return Ok(ResolvedKind::Junction);
                }
                copy_recursive(sys, &pair.source_abs, &pair.dest_abs).await?;
            } else if sys
                .fs_hard_link_async(&pair.source_abs, &pair.dest_abs)
                .await
                .is_ok()
            {
                return Ok(ResolvedKind::Hardlink);
            } else {
                sys.fs_copy_async(&pair.source_abs, &pair.dest_abs).await?;
            }
            Ok(ResolvedKind::Copy)
        }
        LinkKind::Symlink => {
            try_symlink(sys, &rel, &pair.dest_abs, src_is_dir).await?;
            Ok(ResolvedKind::Symlink)
        }
        LinkKind::Junction => {
            if !src_is_dir {
                return Err(ProjectionError::custom(
                    "the `junction` link kind requires a directory source",
                ));
            }
            sys.fs_create_junction_async(&pair.source_abs, &pair.dest_abs)
                .await?;
            Ok(ResolvedKind::Junction)
        }
        LinkKind::Hardlink => {
            if src_is_dir {
                return Err(ProjectionError::custom(
                    "the `hardlink` link kind requires a file source",
                ));
            }
            sys.fs_hard_link_async(&pair.source_abs, &pair.dest_abs)
                .await?;
            Ok(ResolvedKind::Hardlink)
        }
        LinkKind::Copy => {
            if src_is_dir {
                copy_recursive(sys, &pair.source_abs, &pair.dest_abs).await?;
            } else {
                sys.fs_copy_async(&pair.source_abs, &pair.dest_abs).await?;
            }
            Ok(ResolvedKind::Copy)
        }
    }
}

async fn try_symlink<S: ApplierSys>(
    sys: &S,
    rel_target: &Path,
    dest: &Path,
    is_dir: bool,
) -> Result<()> {
    if is_dir {
        sys.fs_symlink_dir_async(rel_target, dest).await?;
    } else {
        sys.fs_symlink_file_async(rel_target, dest).await?;
    }
    Ok(())
}

/// Iteratively copy a directory tree (used only as the `auto`/`copy` fallback).
async fn copy_recursive<S: ApplierSys>(
    sys: &S,
    src: &Path,
    dst: &Path,
) -> Result<()> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];

    while let Some((from, to)) = stack.pop() {
        sys.fs_create_dir_all_async(&to).await?;
        for entry in sys.fs_read_dir_async(&from).await? {
            let name = match entry.file_name() {
                Some(name) => name.to_owned(),
                None => continue,
            };
            let child_to = to.join(&name);
            if sys.fs_is_dir_no_err_async(&entry).await {
                stack.push((entry, child_to));
            } else {
                sys.fs_copy_async(&entry, &child_to).await?;
            }
        }
    }

    Ok(())
}

async fn remove_existing<S: ApplierSys>(sys: &S, path: &Path) -> Result<()> {
    let Some(file_type) = symlink_meta(sys, path).await? else {
        return Ok(());
    };

    match file_type {
        FileType::Symlink => {
            // A symlink/junction may point at a file or a directory.
            if sys.fs_remove_file_async(path).await.is_err() {
                sys.fs_remove_dir_all_async(path).await?;
            }
        }
        FileType::Dir => sys.fs_remove_dir_all_async(path).await?,
        FileType::File | FileType::Unknown => {
            sys.fs_remove_file_async(path).await?
        }
    }

    Ok(())
}

async fn symlink_meta<S: ApplierSys>(
    sys: &S,
    path: &Path,
) -> Result<Option<FileType>> {
    match sys.fs_symlink_metadata_async(path).await {
        Ok(meta) => Ok(Some(meta.file_type())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn relative_target(dest_abs: &Path, source_abs: &Path) -> PathBuf {
    let dest_parent = dest_abs.parent().unwrap_or_else(|| Path::new(""));
    pathdiff::diff_paths(source_abs, dest_parent)
        .unwrap_or_else(|| source_abs.to_path_buf())
}

fn backup_path(dest_abs: &Path) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = dest_abs
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = dest_abs.parent().unwrap_or_else(|| Path::new(""));
    parent.join(format!("{name}.bak.{ts}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_traits::{FsReadLinkAsync, impls::RealSys};

    fn opts(link: LinkKind, collision: CollisionPolicy) -> ApplyOptions {
        ApplyOptions { link, collision }
    }

    #[tokio::test]
    async fn creates_relative_symlink_for_file_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src/file.txt");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"hi").unwrap();
        let dest = dir.path().join("dest/link.txt");

        let sys = RealSys;
        let pair = LinkPair {
            source_abs: src.clone(),
            dest_abs: dest.clone(),
        };
        let outcome = apply_link(
            &sys,
            &pair,
            &opts(LinkKind::Auto, CollisionPolicy::Backup),
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            outcome,
            ApplyOutcome::Linked {
                kind: ResolvedKind::Symlink,
                backup: None
            }
        );
        // The stored target is relative, not absolute.
        let target = sys.fs_read_link_async(&dest).await.unwrap();
        assert!(target.is_relative(), "symlink target should be relative");
        assert_eq!(std::fs::read(&dest).unwrap(), b"hi");
    }

    #[tokio::test]
    async fn backup_collision_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        std::fs::write(&src, b"new").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"old").unwrap();

        let sys = RealSys;
        let pair = LinkPair {
            source_abs: src,
            dest_abs: dest.clone(),
        };
        let outcome = apply_link(
            &sys,
            &pair,
            &opts(LinkKind::Auto, CollisionPolicy::Backup),
            None,
        )
        .await
        .unwrap();

        match outcome {
            ApplyOutcome::Linked {
                backup: Some(path), ..
            } => {
                assert_eq!(std::fs::read(&path).unwrap(), b"old");
            }
            other => panic!("expected backup, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn idempotent_resync_skips_when_pin_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        std::fs::write(&src, b"hi").unwrap();
        let dest = dir.path().join("link.txt");

        let sys = RealSys;
        let pair = LinkPair {
            source_abs: src.clone(),
            dest_abs: dest.clone(),
        };

        // First apply creates the link.
        apply_link(
            &sys,
            &pair,
            &opts(LinkKind::Auto, CollisionPolicy::Error),
            None,
        )
        .await
        .unwrap();

        // Second apply with an unchanged pin is a no-op, not an error.
        let prior = PriorLink {
            source_abs: &src,
            pin_unchanged: true,
        };
        let outcome = apply_link(
            &sys,
            &pair,
            &opts(LinkKind::Auto, CollisionPolicy::Error),
            Some(&prior),
        )
        .await
        .unwrap();
        assert_eq!(outcome, ApplyOutcome::Skipped);
    }

    #[tokio::test]
    async fn skip_collision_leaves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        std::fs::write(&src, b"new").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"old").unwrap();

        let sys = RealSys;
        let pair = LinkPair {
            source_abs: src,
            dest_abs: dest.clone(),
        };
        let outcome = apply_link(
            &sys,
            &pair,
            &opts(LinkKind::Auto, CollisionPolicy::Skip),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome, ApplyOutcome::Skipped);
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    }
}
