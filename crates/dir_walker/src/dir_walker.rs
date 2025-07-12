use std::{error::Error, path::Path};

use crate::DirEntry;

pub trait DirWalkerBase {
    type DirEntry: DirEntry;
    type Error: Error;
    type WalkDir: IntoIterator<Item = Result<Self::DirEntry, Self::Error>>;

    fn base_walk_dir(&self, path: &Path) -> Self::WalkDir;
}

pub trait DirWalker: DirWalkerBase {
    #[inline(always)]
    fn walk_dir(&self, path: impl AsRef<Path>) -> Self::WalkDir {
        self.base_walk_dir(path.as_ref())
    }
}

impl<T: DirWalkerBase> DirWalker for T {}
