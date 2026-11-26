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

use super::{OpenError, Want};
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
    _want: Want,
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
    if names.is_empty() {
        // The root itself: another handle on the held root directory.
        return root
            .dir
            .try_clone()
            .map(Dir::into_std_file)
            .map_err(io_error);
    }
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
            check_final(&found, &root.final_path, &names)?;
            return Ok(file);
        }
        if !metadata.is_dir() {
            // A file where a directory is needed: ENOTDIR, so missing (D6).
            return Err(OpenError::Missing);
        }
        retained.push(Dir::from_std_file(file));
        hook(Step::Retained(index));
    }
    unreachable!("the loop returns on the last component")
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

/// The final path of the handle must be exactly `root` plus the requested
/// names. Anything outside the root is an escape. The right place with a
/// different spelling (case, or an 8.3 alias) is a `CaseMismatch` (D8): the
/// view compares names byte for byte and never case-folds. Fails closed on a
/// name that is not valid Unicode.
fn check_final(
    found: &Path,
    root: &Path,
    names: &[&str],
) -> Result<(), OpenError> {
    fn text(component: Component<'_>) -> Option<&str> {
        component.as_os_str().to_str()
    }
    let escape = || OpenError::from(ContainmentViolation::EscapesRoot);
    let mut found = found.components();
    for expected in root.components() {
        let (Some(actual), Some(expected)) =
            (found.next().and_then(text), text(expected))
        else {
            return Err(escape());
        };
        if !actual.eq_ignore_ascii_case(expected)
            && actual.to_lowercase() != expected.to_lowercase()
        {
            return Err(escape());
        }
    }
    let rest: Vec<Component<'_>> = found.collect();
    if rest.len() != names.len() {
        return Err(escape());
    }
    for (actual, expected) in rest.into_iter().zip(names) {
        match text(actual) {
            Some(actual) if actual == *expected => {}
            Some(_) => return Err(OpenError::CaseMismatch),
            None => return Err(escape()),
        }
    }
    Ok(())
}

pub(crate) struct RawEntry {
    pub(crate) name: Result<String, ()>,
    pub(crate) kind: crate::error::EntryKind,
}

/// Entries of an open directory, kinds taken without following links. cap-std
/// lists through the handle's path, so a directory deeper than `MAX_PATH` on a
/// system without long paths enabled cannot be listed even though it can be
/// opened.
pub(crate) fn entries(
    dir: File,
) -> std::io::Result<impl Iterator<Item = std::io::Result<RawEntry>>> {
    use crate::error::EntryKind;
    let reader = Dir::from_std_file(dir).entries()?;
    Ok(reader.map(|entry| {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let kind = if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_dir() {
            EntryKind::Dir
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        Ok(RawEntry {
            name: entry.file_name().into_string().map_err(|_| ()),
            kind,
        })
    }))
}

fn io_error(error: std::io::Error) -> OpenError {
    match error.kind() {
        std::io::ErrorKind::NotFound => OpenError::Missing,
        kind => OpenError::Io(kind),
    }
}

#[cfg(test)]
mod tests;
