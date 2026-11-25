//! Beneath-root open primitive. One implementation per OS family; the shared
//! layer here owns name validation (D9) and error mapping so every OS reports
//! the same way.

use std::{fs::File, io, path::Path};

use omni_path_utils::WorkspacePath;

use crate::{
    error::{ContainmentViolation, InputError},
    name::check_path,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

#[cfg(not(any(target_os = "linux", windows)))]
mod unsupported;
#[cfg(not(any(target_os = "linux", windows)))]
use unsupported as imp;

/// Result of the per-OS open, before the shared mapping to [`InputError`].
/// The unsupported OS family only ever produces `Violation`.
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OpenError {
    /// `ENOENT` or `ENOTDIR` (D6): the path does not exist.
    Missing,
    Violation(ContainmentViolation),
    Io(io::ErrorKind),
}

impl From<ContainmentViolation> for OpenError {
    fn from(violation: ContainmentViolation) -> Self {
        Self::Violation(violation)
    }
}

/// A held handle to the workspace root. All access is relative to it.
pub struct ContainedRoot(imp::Root);

impl ContainedRoot {
    /// Canonicalizes `root` once and holds a handle to it. On platforms
    /// without verified containment this returns
    /// [`ContainmentViolation::Unsupported`] without touching the filesystem.
    pub fn open(root: &Path) -> Result<Self, InputError> {
        imp::Root::open(root)
            .map(Self)
            .map_err(|error| map_error(error, &WorkspacePath::root()))
    }
}

/// F10.2a proof fixture: open a regular path beneath the root and return the
/// file. `Ok(None)` is a missing path. Any symlink or reparse point on the
/// path is an error, never a missing path. The D9 name check runs first, before
/// any filesystem access.
///
/// Hidden: the view returns no handles to consumers. F10.2b replaces this with
/// the `InputView` API.
#[doc(hidden)]
pub fn open_beneath(
    root: &ContainedRoot,
    path: &WorkspacePath,
) -> Result<Option<File>, InputError> {
    open_beneath_with(&root.0, path, imp::open_file)
}

/// The shared layer around the per-OS open: the request and name checks must
/// run before `open` is called at all.
fn open_beneath_with<R>(
    root: &R,
    path: &WorkspacePath,
    open: impl FnOnce(&R, &WorkspacePath) -> Result<File, OpenError>,
) -> Result<Option<File>, InputError> {
    check_open_request(path)?;
    match open(root, path) {
        Ok(file) => Ok(Some(file)),
        Err(OpenError::Missing) => Ok(None),
        Err(error) => Err(map_error(error, path)),
    }
}

fn check_open_request(path: &WorkspacePath) -> Result<(), InputError> {
    if path.is_root() {
        return Err(InputError::Io {
            path: path.clone(),
            kind: io::ErrorKind::InvalidInput,
        });
    }
    check_path(path)?;
    Ok(())
}

fn map_error(error: OpenError, path: &WorkspacePath) -> InputError {
    match error {
        OpenError::Missing => InputError::Io {
            path: path.clone(),
            kind: io::ErrorKind::NotFound,
        },
        OpenError::Violation(violation) => InputError::Containment(violation),
        OpenError::Io(kind) => InputError::Io {
            path: path.clone(),
            kind,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn rejected_requests_never_reach_the_os_open() {
        let calls = Cell::new(0);
        let attempt = |path: WorkspacePath| {
            open_beneath_with(&(), &path, |_, _| {
                calls.set(calls.get() + 1);
                Err(OpenError::Missing)
            })
        };
        for bad in ["ab:c", "a/CON", "nul.txt", "a/b./c", "a/trail ", "a/COM0"]
        {
            let error =
                attempt(WorkspacePath::parse(bad).unwrap()).unwrap_err();
            assert!(
                matches!(
                    error,
                    InputError::Containment(
                        ContainmentViolation::NameUnsupported
                    )
                ),
                "{bad}: {error:?}"
            );
        }
        assert!(attempt(WorkspacePath::root()).is_err());
        assert_eq!(calls.get(), 0);
        assert!(
            attempt(WorkspacePath::parse("a/b").unwrap())
                .unwrap()
                .is_none()
        );
        assert_eq!(calls.get(), 1);
    }
}
