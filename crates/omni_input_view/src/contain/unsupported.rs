//! macOS and every other OS: no verified beneath-root open (D5), so nothing is
//! walked and nothing is read.

use std::{fs::File, path::Path};

use omni_path_utils::WorkspacePath;

use super::{OpenError, Want};
use crate::error::ContainmentViolation;

pub(crate) struct Root;

impl Root {
    pub(crate) fn open(_root: &Path) -> Result<Self, OpenError> {
        Err(ContainmentViolation::Unsupported.into())
    }
}

pub(crate) fn open_file(
    _root: &Root,
    _path: &WorkspacePath,
    _want: Want,
) -> Result<File, OpenError> {
    Err(ContainmentViolation::Unsupported.into())
}

pub(crate) struct RawEntry {
    pub(crate) name: Result<String, ()>,
    pub(crate) kind: crate::error::EntryKind,
}

pub(crate) fn entries(
    _dir: File,
) -> std::io::Result<impl Iterator<Item = std::io::Result<RawEntry>>> {
    Err::<std::iter::Empty<_>, _>(std::io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use crate::{ContainedRoot, error::InputError};

    #[test]
    fn root_is_unsupported_without_touching_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f"), b"x").unwrap();
        for root in [dir.path(), std::path::Path::new("/does/not/exist")] {
            assert!(matches!(
                ContainedRoot::open(root),
                Err(InputError::ContainmentUnsupported)
            ));
        }
    }
}
