//! Windows: a walk this module owns, one component per open, over retained
//! handles.
//!
//! - Every component is opened handle-relative with `Dir::open_with`, with
//!   `FILE_FLAG_OPEN_REPARSE_POINT` so a link is returned as itself and never
//!   followed (cap-std's own no-follow enforcement is off when the flag is
//!   present), and with a share mode that omits `FILE_SHARE_DELETE`.
//! - Every ancestor handle stays open until the walk ends, so an ancestor
//!   cannot be renamed or deleted underneath it (verified by the Windows CI
//!   job, see the tests).
//! - Any reparse point (symlink, junction, mount point, and also placeholders)
//!   is rejected.
//! - The final path of the opened handle must still be under the root's.

use std::{
    fs::File,
    os::windows::fs::{MetadataExt, OpenOptionsExt as StdOpenOptionsExt},
    path::{Component, Path},
};

use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
use omni_path_utils::WorkspacePath;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::OpenError;
use crate::error::ContainmentViolation;

mod sys;

const SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;

pub(crate) struct Root {
    dir: Dir,
    final_path: std::path::PathBuf,
}

impl Root {
    pub(crate) fn open(root: &Path) -> Result<Self, OpenError> {
        let canonical = std::fs::canonicalize(root).map_err(io_error)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(SHARE)
            .custom_flags(
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            )
            .open(&canonical)
            .map_err(io_error)?;
        let metadata = file.metadata().map_err(io_error)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ContainmentViolation::ReparsePoint.into());
        }
        if !metadata.is_dir() {
            return Err(OpenError::Io(std::io::ErrorKind::NotADirectory));
        }
        let final_path = sys::final_path(&file).map_err(io_error)?;
        Ok(Self {
            dir: Dir::from_std_file(file),
            final_path,
        })
    }
}

/// A point in the walk where tests inject a hook (deterministic variants of the
/// race: the state of the next component can change right here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Component `index` was opened and retained; the next one is not opened.
    Retained(usize),
    /// The final component was opened; the final-path check has not run.
    Opened,
}

pub(crate) fn open_file(
    root: &Root,
    path: &WorkspacePath,
) -> Result<File, OpenError> {
    open_file_with(root, path, &mut |_| {})
}

pub(crate) fn open_file_with(
    root: &Root,
    path: &WorkspacePath,
    hook: &mut dyn FnMut(Step),
) -> Result<File, OpenError> {
    open_file_with_flags(root, path, hook, FILE_FLAG_OPEN_REPARSE_POINT)
}

/// `reparse_flag` is a parameter only so a test can omit
/// `FILE_FLAG_OPEN_REPARSE_POINT` and prove it is load-bearing.
pub(crate) fn open_file_with_flags(
    root: &Root,
    path: &WorkspacePath,
    hook: &mut dyn FnMut(Step),
    reparse_flag: u32,
) -> Result<File, OpenError> {
    let options = open_options(reparse_flag);
    let names: Vec<&str> = path.segments().collect();
    let mut retained: Vec<Dir> = Vec::with_capacity(names.len());

    for (index, name) in names.iter().enumerate() {
        let last = index + 1 == names.len();
        let file = {
            let base = retained.last().unwrap_or(&root.dir);
            single_component(name)?;
            base.open_with(name, &options).map_err(io_error)?.into_std()
        };
        let metadata = file.metadata().map_err(io_error)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ContainmentViolation::ReparsePoint.into());
        }
        if last {
            hook(Step::Opened);
            let found = sys::final_path(&file).map_err(io_error)?;
            if !is_under(&found, &root.final_path) {
                return Err(ContainmentViolation::EscapesRoot.into());
            }
            return Ok(file);
        }
        if !metadata.is_dir() {
            // A file where a directory is needed: ENOTDIR, so missing (D6).
            return Err(OpenError::Missing);
        }
        retained.push(Dir::from_std_file(file));
        hook(Step::Retained(index));
    }
    Err(OpenError::Io(std::io::ErrorKind::InvalidInput))
}

fn open_options(reparse_flag: u32) -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(SHARE)
        .custom_flags(reparse_flag | FILE_FLAG_BACKUP_SEMANTICS);
    options
}

/// Exactly one normal component per `open_with`: more would make cap-std walk
/// the rest itself and drop the intermediate handles.
fn single_component(name: &str) -> Result<(), OpenError> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(OpenError::Io(std::io::ErrorKind::InvalidInput)),
    }
}

/// Strictly beneath `root`, compared case-insensitively. Fails closed on a
/// name that is not valid Unicode.
fn is_under(found: &Path, root: &Path) -> bool {
    fn folded(component: Component<'_>) -> Option<String> {
        component.as_os_str().to_str().map(str::to_lowercase)
    }
    let mut found = found.components();
    let same_prefix = root.components().all(|expected| {
        match (found.next().and_then(folded), folded(expected)) {
            (Some(actual), Some(expected)) => actual == expected,
            _ => false,
        }
    });
    same_prefix && found.next().is_some()
}

fn io_error(error: std::io::Error) -> OpenError {
    match error.kind() {
        std::io::ErrorKind::NotFound => OpenError::Missing,
        kind => OpenError::Io(kind),
    }
}

#[cfg(test)]
mod tests;
