use std::{error::Error, path::Path};

use crate::DirEntry;

pub trait DirWalkerBase {
    type DirEntry: DirEntry;
    type Error: Error;
    type IterError: Error;
    type WalkDir: IntoIterator<Item = Result<Self::DirEntry, Self::IterError>>;

    fn base_walk_dir(
        &self,
        paths: &[&Path],
    ) -> Result<Self::WalkDir, Self::Error>;
}

pub trait DirWalker: DirWalkerBase {
    #[inline(always)]
    fn walk_dir(
        &self,
        paths: &[impl AsRef<Path>],
    ) -> Result<Self::WalkDir, Self::Error> {
        self.base_walk_dir(
            &paths.iter().map(|p| p.as_ref()).collect::<Vec<_>>(),
        )
    }
}

impl<T: DirWalkerBase> DirWalker for T {}
