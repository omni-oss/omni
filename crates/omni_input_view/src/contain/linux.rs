//! Linux: a single `openat2` relative to the held root fd. The kernel enforces
//! the boundary for the whole lookup, so there is no window between a check and
//! the open, and renaming an intermediate directory away and back cannot make
//! the lookup leave the root. There is no fallback walk: without `openat2` the
//! answer is `Unsupported` and nothing is read.

use std::{
    fs::File,
    os::fd::{AsFd, BorrowedFd, OwnedFd},
    path::Path,
};

use omni_path_utils::WorkspacePath;
use rustix::{
    fs::{self, Mode, OFlags, ResolveFlags},
    io::Errno,
};

use super::OpenError;
use crate::error::ContainmentViolation;

/// Bounded retries for `EAGAIN` (a racing rename) and `EINTR`.
const MAX_RETRIES: usize = 4;

pub(crate) struct Root {
    fd: OwnedFd,
}

impl Root {
    pub(crate) fn open(root: &Path) -> Result<Self, OpenError> {
        let canonical = std::fs::canonicalize(root)
            .map_err(|error| OpenError::Io(error.kind()))?;
        let fd = fs::open(
            &canonical,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|errno| OpenError::Io(std::io::Error::from(errno).kind()))?;
        Ok(Self { fd })
    }
}

/// The syscall seam is a closure with the shape of [`real_openat2`]; tests
/// inject errno values and hooks that run just before the kernel lookup.
fn real_openat2(
    dirfd: BorrowedFd<'_>,
    path: &str,
    oflags: OFlags,
    resolve: ResolveFlags,
) -> Result<OwnedFd, Errno> {
    fs::openat2(dirfd, path, oflags, Mode::empty(), resolve)
}

pub(crate) fn open_file(
    root: &Root,
    path: &WorkspacePath,
) -> Result<File, OpenError> {
    open_file_with(root, path, &real_openat2)
}

pub(crate) fn open_file_with<F>(
    root: &Root,
    path: &WorkspacePath,
    openat2: &F,
) -> Result<File, OpenError>
where
    F: Fn(BorrowedFd<'_>, &str, OFlags, ResolveFlags) -> Result<OwnedFd, Errno>,
{
    let oflags =
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::NOCTTY;
    let resolve = ResolveFlags::BENEATH
        | ResolveFlags::NO_SYMLINKS
        | ResolveFlags::NO_MAGICLINKS
        | ResolveFlags::NO_XDEV;
    let relative = if path.is_root() { "." } else { path.as_str() };

    let mut attempts = 0;
    loop {
        match openat2(root.fd.as_fd(), relative, oflags, resolve) {
            Ok(fd) => return Ok(File::from(fd)),
            Err(Errno::AGAIN | Errno::INTR) if attempts < MAX_RETRIES => {
                attempts += 1;
            }
            Err(errno) => return Err(map_errno(errno)),
        }
    }
}

/// `EXDEV` is reported as [`ContainmentViolation::MountCrossing`]. The kernel
/// uses it for two causes it does not tell apart: a mount crossing
/// (`RESOLVE_NO_XDEV`) and a `RESOLVE_BENEATH` escape it detects when a
/// directory on the path is renamed during the lookup (observed in the
/// move-out-and-back race). Both are refusals, so this is safe; the variant
/// name only says what the common cause is.
pub(crate) fn map_errno(errno: Errno) -> OpenError {
    match errno {
        Errno::NOENT | Errno::NOTDIR => OpenError::Missing,
        Errno::LOOP => ContainmentViolation::Symlink.into(),
        Errno::XDEV => ContainmentViolation::MountCrossing.into(),
        Errno::NOSYS | Errno::PERM => ContainmentViolation::Unsupported.into(),
        Errno::AGAIN | Errno::INTR => {
            OpenError::Io(std::io::ErrorKind::WouldBlock)
        }
        other => OpenError::Io(std::io::Error::from(other).kind()),
    }
}

#[cfg(test)]
mod tests;
