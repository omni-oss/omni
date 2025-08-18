use std::{
    error::Error,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::Metadata;

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

    fn is_dir(&self) -> bool {
        self.metadata().map(|m| m.is_dir()).unwrap_or(false)
    }

    fn is_file(&self) -> bool {
        self.metadata().map(|m| m.is_file()).unwrap_or(false)
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
