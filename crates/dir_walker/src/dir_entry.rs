use std::{
    error::Error,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::Metadata;

/// The kind of a directory entry, as reported cheaply by the walker
/// (typically the `readdir`/`getdents` `d_type` field) without an extra
/// `stat`/`statx` syscall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Dir,
    File,
    Symlink,
    Other,
}

pub trait DirEntry {
    type Error: Error + Send + Sync + 'static;
    type Metadata: Metadata;

    /// The full path that this entry represents.
    fn path(&self) -> &Path;

    /// The full path that this entry represents.
    /// Analogous to [`DirEntry::path`], but moves ownership of the path.
    fn into_path(self) -> PathBuf;

    /// Whether this entry corresponds to a symbolic link or not.
    fn path_is_symlink(&self) -> bool;

    /// Return the metadata for the file that this entry points to.
    fn metadata(&self) -> Result<Self::Metadata, Self::Error>;

    /// The file type of this entry, if it can be determined *without* an
    /// extra syscall. Walkers backed by `readdir`/`getdents` return this
    /// cheaply; `None` means a `stat` would be required (e.g. the traversal
    /// root or a filesystem that doesn't fill `d_type`).
    ///
    /// Like `std::fs::DirEntry::file_type`, this does not follow symlinks.
    fn file_type(&self) -> Option<FileType> {
        None
    }

    fn is_dir(&self) -> bool {
        match self.file_type() {
            // Fast path: type known from the directory read, no syscall.
            Some(FileType::Dir) => true,
            Some(FileType::File) | Some(FileType::Other) => false,
            // Preserve old behavior: symlinks (and unknown) still stat and
            // follow the link.
            Some(FileType::Symlink) | None => {
                self.metadata().map(|m| m.is_dir()).unwrap_or(false)
            }
        }
    }

    fn is_file(&self) -> bool {
        match self.file_type() {
            Some(FileType::File) => true,
            Some(FileType::Dir) | Some(FileType::Other) => false,
            Some(FileType::Symlink) | None => {
                self.metadata().map(|m| m.is_file()).unwrap_or(false)
            }
        }
    }

    /// Return the file name of this entry.
    ///
    /// If this entry has no file name (e.g., `/`), then the full path is
    /// returned.
    fn file_name(&self) -> &OsStr;

    /// Returns the depth at which this entry was created relative to the root.
    fn depth(&self) -> usize;

    /// Returns the underlying inode number if one exists.
    ///
    /// If this entry doesn't have an inode number, then `None` is returned.
    #[cfg(unix)]
    fn ino(&self) -> Option<u64>;
}
