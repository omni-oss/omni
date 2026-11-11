use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use omni_projection_configurations::{ExistingPolicy, LinkKind};
use system_traits::{
    FileType, FsCopyAsync, FsCreateDirAllAsync, FsCreateJunctionAsync,
    FsHardLinkAsync, FsMetadataAsync, FsMetadataValue, FsReadDirAsync,
    FsReadLinkAsync, FsReflinkAsync, FsRemoveDirAllAsync, FsRemoveFileAsync,
    FsRenameAsync, FsSymlinkDirAsync, FsSymlinkFileAsync,
};

use crate::error::{ProjectionError, Result};
use crate::routing::LinkPair;

/// The link kind that was actually materialized (after `auto` fallback).
#[derive(
    serde::Serialize,
    serde::Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ResolvedKind {
    Reflink,
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
    pub on_existing: ExistingPolicy,
    /// The source lives in the shared remote-source store (a git source). `auto`
    /// then prefers independent owned files (reflink) over links into a
    /// store that can be deleted, and links into it by absolute path.
    pub from_store: bool,
}

/// A destination omni already owns, per the ledger. Used for the idempotent
/// "up-to-date" exception: an unchanged pin means the link is left untouched;
/// a changed pin means omni re-points it (regardless of the existing-file policy).
pub struct PriorLink<'a> {
    pub source_abs: &'a Path,
    pub pin_unchanged: bool,
}

/// Whether a destination is a foreign existing file: it is present on disk and
/// the ledger does not record omni as its owner for this source. An omni-owned
/// destination (matching source, whichever pin) is never foreign; omni re-points
/// or leaves it and never treats it as a conflict. Shared by `apply_link` and the
/// run-wide existing-file check so the two cannot disagree.
pub fn is_foreign_existing(
    dest_exists: bool,
    source_abs: &Path,
    prior: Option<&PriorLink<'_>>,
) -> bool {
    if !dest_exists {
        return false;
    }
    !matches!(prior, Some(p) if p.source_abs == source_abs)
}

/// The full trait surface the applier needs from a system.
pub trait ApplierSys:
    FsSymlinkFileAsync
    + FsSymlinkDirAsync
    + FsCreateJunctionAsync
    + FsHardLinkAsync
    + FsReadLinkAsync
    + FsReflinkAsync
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
        + FsReadLinkAsync
        + FsReflinkAsync
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

/// Materialize one link, honoring the ownership exception and existing-file policy.
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
    if let Some(prior) = prior
        && prior.source_abs == pair.source_abs
        && dest_exists
    {
        // An unchanged pin leaves a healthy link alone. A link that no longer
        // resolves (its store checkout was deleted and has since been
        // refetched) is repaired like a changed pin.
        if prior.pin_unchanged
            && sys.fs_metadata_async(&pair.dest_abs).await.is_ok()
        {
            return Ok(ApplyOutcome::Skipped);
        }
        // Pin changed or link dangling: re-point the link omni owns, no backup.
        remove_existing(sys, &pair.dest_abs).await?;
        let kind = materialize(sys, pair, opts).await?;
        return Ok(ApplyOutcome::Linked { kind, backup: None });
    }

    let mut backup = None;
    if is_foreign_existing(dest_exists, &pair.source_abs, prior) {
        match opts.on_existing {
            ExistingPolicy::Skip => return Ok(ApplyOutcome::Skipped),
            ExistingPolicy::Error => {
                return Err(ProjectionError::custom(format!(
                    "destination already exists: {}",
                    pair.dest_abs.display()
                )));
            }
            ExistingPolicy::Overwrite => {
                remove_existing(sys, &pair.dest_abs).await?;
            }
            ExistingPolicy::Backup => {
                let path = backup_path(&pair.dest_abs);
                sys.fs_rename_async(&pair.dest_abs, &path).await?;
                backup = Some(path);
            }
        }
    }

    let kind = materialize(sys, pair, opts).await?;
    Ok(ApplyOutcome::Linked { kind, backup })
}

async fn materialize<S: ApplierSys>(
    sys: &S,
    pair: &LinkPair,
    opts: &ApplyOptions,
) -> Result<ResolvedKind> {
    let src_is_dir = sys.fs_is_dir_no_err_async(&pair.source_abs).await;
    // A link into the shared store is absolute: a relative one would break
    // when the workspace moves, and the store is far from the workspace.
    let symlink_target = if opts.from_store {
        pair.source_abs.clone()
    } else {
        relative_target(&pair.dest_abs, &pair.source_abs)
    };

    match opts.link {
        LinkKind::Auto if opts.from_store => {
            // The clone call reports a missing source as an invalid argument,
            // which would read as "unsupported" and end in a dangling link.
            if !sys.fs_exists_no_err_async(&pair.source_abs).await {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "projection source not found: {}",
                        pair.source_abs.display()
                    ),
                )
                .into());
            }
            // Owned, independent files first: they survive a deleted store and
            // need no sandbox read grant. Hard links are never chosen here,
            // because editing one in place would change the shared tree.
            match link_files(sys, pair, src_is_dir, FileOp::Reflink).await {
                Ok(()) => {
                    make_owned_writable(&pair.dest_abs);
                    return Ok(ResolvedKind::Reflink);
                }
                Err(error) if is_situational(&error) => {}
                Err(error) => return Err(error.into()),
            }
            // A directory is never linked into the store: a directory symlink
            // lets the workspace add or replace files in the shared tree, which
            // read-only files cannot prevent. It is copied instead.
            if !src_is_dir
                && let Some(kind) =
                    auto_symlink(sys, &symlink_target, pair, src_is_dir).await?
            {
                return Ok(kind);
            }
            let kind = copy(sys, pair, src_is_dir).await?;
            make_owned_writable(&pair.dest_abs);
            Ok(kind)
        }
        LinkKind::Auto => {
            if let Some(kind) =
                auto_symlink(sys, &symlink_target, pair, src_is_dir).await?
            {
                return Ok(kind);
            }
            if !src_is_dir
                && sys
                    .fs_hard_link_async(&pair.source_abs, &pair.dest_abs)
                    .await
                    .is_ok()
            {
                return Ok(ResolvedKind::Hardlink);
            }
            copy(sys, pair, src_is_dir).await
        }
        LinkKind::Symlink => {
            try_symlink(sys, &symlink_target, &pair.dest_abs, src_is_dir)
                .await?;
            Ok(ResolvedKind::Symlink)
        }
        LinkKind::Hardlink => {
            // Strict: an error when the files cannot be hard linked.
            link_files(sys, pair, src_is_dir, FileOp::Hardlink).await?;
            Ok(ResolvedKind::Hardlink)
        }
        LinkKind::Copy => {
            let kind = copy(sys, pair, src_is_dir).await?;
            if opts.from_store {
                make_owned_writable(&pair.dest_abs);
            }
            Ok(kind)
        }
    }
}

/// Store files are read-only so a hard link or symlink cannot edit the shared
/// tree. A copy or reflink is the workspace's own file, so give it back its
/// write permission. Best effort: a failure leaves a read-only file, never a
/// failed projection.
fn make_owned_writable(path: &Path) {
    fn visit(path: &Path) {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return;
        };
        if meta.is_symlink() {
            return;
        }
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    visit(&entry.path());
                }
            }
            return;
        }
        let mut permissions = meta.permissions();
        if permissions.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = std::fs::set_permissions(path, permissions);
        }
    }
    visit(path);
}

/// The `auto` symlink step. `Ok(None)` means symlinks are unavailable here
/// (unsupported, or no privilege) and the caller should try the next method;
/// any other failure is returned. A directory gets a junction as a last link
/// before copying.
async fn auto_symlink<S: ApplierSys>(
    sys: &S,
    target: &Path,
    pair: &LinkPair,
    is_dir: bool,
) -> Result<Option<ResolvedKind>> {
    match try_symlink(sys, target, &pair.dest_abs, is_dir).await {
        Ok(()) => return Ok(Some(ResolvedKind::Symlink)),
        Err(error) if symlink_unavailable(&error) => {}
        Err(error) => return Err(error.into()),
    }
    if is_dir
        && sys
            .fs_create_junction_async(&pair.source_abs, &pair.dest_abs)
            .await
            .is_ok()
    {
        return Ok(Some(ResolvedKind::Junction));
    }
    Ok(None)
}

async fn copy<S: ApplierSys>(
    sys: &S,
    pair: &LinkPair,
    src_is_dir: bool,
) -> Result<ResolvedKind> {
    if src_is_dir {
        copy_recursive(sys, &pair.source_abs, &pair.dest_abs).await?;
    } else {
        sys.fs_copy_async(&pair.source_abs, &pair.dest_abs).await?;
    }
    Ok(ResolvedKind::Copy)
}

#[derive(Clone, Copy)]
enum FileOp {
    Reflink,
    Hardlink,
}

impl FileOp {
    async fn apply<S: ApplierSys>(
        self,
        sys: &S,
        src: &Path,
        dst: &Path,
    ) -> std::io::Result<()> {
        match self {
            Self::Reflink => sys.fs_reflink_async(src, dst).await,
            Self::Hardlink => sys.fs_hard_link_async(src, dst).await,
        }
    }
}

/// Link a file, or a directory file by file, with `op`. All or nothing: when
/// any file fails, whatever was created is removed so no partial tree is left
/// behind, and the error is returned for the caller to classify.
async fn link_files<S: ApplierSys>(
    sys: &S,
    pair: &LinkPair,
    src_is_dir: bool,
    op: FileOp,
) -> std::io::Result<()> {
    if !src_is_dir {
        return op.apply(sys, &pair.source_abs, &pair.dest_abs).await;
    }

    let result = link_tree(sys, &pair.source_abs, &pair.dest_abs, op).await;
    if result.is_err() {
        let _ = sys.fs_remove_dir_all_async(&pair.dest_abs).await;
    }
    result
}

async fn link_tree<S: ApplierSys>(
    sys: &S,
    src: &Path,
    dst: &Path,
    op: FileOp,
) -> std::io::Result<()> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];

    while let Some((from, to)) = stack.pop() {
        sys.fs_create_dir_all_async(&to).await?;
        for entry in sys.fs_read_dir_async(&from).await? {
            let Some(name) = entry.file_name().map(|n| n.to_owned()) else {
                continue;
            };
            let child_to = to.join(&name);
            if copy_symlink(sys, &entry, &child_to).await? {
                continue;
            }
            if sys.fs_is_dir_no_err_async(&entry).await {
                stack.push((entry, child_to));
            } else {
                op.apply(sys, &entry, &child_to).await?;
            }
        }
    }

    Ok(())
}

/// A symlink inside a source tree is reproduced as a symlink, never followed:
/// following one would read (and copy or link) whatever it points at, including
/// files outside the source root. Returns whether `entry` was a symlink.
async fn copy_symlink<S: ApplierSys>(
    sys: &S,
    entry: &Path,
    to: &Path,
) -> std::io::Result<bool> {
    let is_symlink = matches!(
        sys.fs_symlink_metadata_async(entry)
            .await
            .map(|meta| meta.file_type()),
        Ok(FileType::Symlink)
    );
    if !is_symlink {
        return Ok(false);
    }
    let target = sys.fs_read_link_async(entry).await?;
    let points_at_dir = sys.fs_is_dir_no_err_async(entry).await;
    try_symlink(sys, &target, to, points_at_dir).await?;
    Ok(true)
}

/// Errors that mean "this method cannot work here" (the volume, platform or
/// privileges do not allow it), as opposed to a real failure. `auto` falls
/// through to the next method only on these; anything else (permission denied,
/// out of space, not found, I/O) is returned.
pub fn is_situational(error: &std::io::Error) -> bool {
    use std::io::ErrorKind::{CrossesDevices, InvalidInput, Unsupported};

    if matches!(error.kind(), Unsupported | CrossesDevices | InvalidInput) {
        return true;
    }
    match error.raw_os_error() {
        // ENOTTY: ext4 and friends answer a clone ioctl this way.
        #[cfg(unix)]
        Some(25) => true,
        // INVALID_FUNCTION, NOT_SUPPORTED, PRIVILEGE_NOT_HELD.
        #[cfg(windows)]
        Some(1 | 50 | 1314) => true,
        _ => false,
    }
}

/// Like [`is_situational`], for creating a symlink. POSIX reports a filesystem
/// that cannot hold symlinks (FAT, exFAT) as `EPERM`.
fn symlink_unavailable(error: &std::io::Error) -> bool {
    is_situational(error)
        || (cfg!(unix) && error.kind() == std::io::ErrorKind::PermissionDenied)
}

async fn try_symlink<S: ApplierSys>(
    sys: &S,
    rel_target: &Path,
    dest: &Path,
    is_dir: bool,
) -> std::io::Result<()> {
    if is_dir {
        sys.fs_symlink_dir_async(rel_target, dest).await
    } else {
        sys.fs_symlink_file_async(rel_target, dest).await
    }
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
            if copy_symlink(sys, &entry, &child_to)
                .await
                .map_err(ProjectionError::from)?
            {
                continue;
            }
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

pub(crate) async fn symlink_meta<S: ApplierSys>(
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

    fn opts(link: LinkKind, on_existing: ExistingPolicy) -> ApplyOptions {
        ApplyOptions {
            link,
            on_existing,
            from_store: false,
        }
    }

    #[test]
    fn only_situational_errors_let_auto_fall_through() {
        use std::io::{Error, ErrorKind};

        for kind in [
            ErrorKind::Unsupported,
            ErrorKind::CrossesDevices,
            ErrorKind::InvalidInput,
        ] {
            assert!(is_situational(&Error::from(kind)), "{kind:?}");
        }
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::NotFound,
            ErrorKind::StorageFull,
            ErrorKind::Other,
            ErrorKind::AlreadyExists,
        ] {
            assert!(!is_situational(&Error::from(kind)), "{kind:?}");
        }
        // POSIX reports "this filesystem has no symlinks" as EPERM.
        let denied = Error::from(ErrorKind::PermissionDenied);
        assert_eq!(symlink_unavailable(&denied), cfg!(unix));
        assert!(!symlink_unavailable(&Error::from(ErrorKind::NotFound)));
        #[cfg(windows)]
        assert!(is_situational(&Error::from_raw_os_error(1314)));
    }

    #[test]
    fn foreign_existing_predicate_respects_ownership() {
        let owner = Path::new("/src/a");
        let owned = PriorLink {
            source_abs: owner,
            pin_unchanged: true,
        };
        let owned_changed = PriorLink {
            source_abs: owner,
            pin_unchanged: false,
        };
        let other = PriorLink {
            source_abs: Path::new("/src/other"),
            pin_unchanged: true,
        };

        assert!(!is_foreign_existing(false, owner, None));
        assert!(is_foreign_existing(true, owner, None));
        assert!(!is_foreign_existing(true, owner, Some(&owned)));
        assert!(!is_foreign_existing(true, owner, Some(&owned_changed)));
        assert!(is_foreign_existing(true, owner, Some(&other)));
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
            &opts(LinkKind::Auto, ExistingPolicy::Backup),
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
            &opts(LinkKind::Auto, ExistingPolicy::Backup),
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
            &opts(LinkKind::Auto, ExistingPolicy::Error),
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
            &opts(LinkKind::Auto, ExistingPolicy::Error),
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
            &opts(LinkKind::Auto, ExistingPolicy::Skip),
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome, ApplyOutcome::Skipped);
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
    }
}
