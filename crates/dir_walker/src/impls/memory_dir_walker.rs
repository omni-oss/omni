use std::path::Path;

use derive_more::Constructor;
use derive_new::new;
use thiserror::Error;

use crate::{DirEntry, DirWalkerBase, Metadata};

pub struct InMemoryDirWalker(Vec<InMemoryDirEntry>);

impl InMemoryDirWalker {
    pub fn new(mut entries: Vec<InMemoryDirEntry>) -> Self {
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Self(entries)
    }

    pub fn add(&mut self, entry: InMemoryDirEntry) {
        self.0.push(entry);
    }
}

impl DirWalkerBase for InMemoryDirWalker {
    type DirEntry = InMemoryDirEntry;
    type IterError = InMemoryDirWalkerError;
    type Error = InMemoryDirWalkerError;
    type WalkDir = InMemoryWalkDir;

    fn base_walk_dir(
        &self,
        path: &[&std::path::Path],
    ) -> Result<Self::WalkDir, Self::Error> {
        Ok(InMemoryWalkDir::new(path, &self.0))
    }
}

#[derive(Clone, Debug, Constructor)]
pub struct InMemoryDirEntry {
    path: std::path::PathBuf,
    is_symlink: bool,
    metadata: InMemoryMetadata,
}

/// Doesn't do anything for now
#[derive(Clone, Debug, Default, new)]
pub struct InMemoryMetadata {
    dir: bool,
    file: bool,
}

impl Metadata for InMemoryMetadata {
    fn is_dir(&self) -> bool {
        self.dir
    }

    fn is_file(&self) -> bool {
        self.file
    }
}

#[derive(Error, Debug)]
#[error("InMemoryDirWalkerError")]
pub struct InMemoryDirWalkerError;

impl DirEntry for InMemoryDirEntry {
    type Error = InMemoryDirWalkerError;

    type Metadata = InMemoryMetadata;

    fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn into_path(self) -> std::path::PathBuf {
        self.path
    }

    fn path_is_symlink(&self) -> bool {
        self.is_symlink
    }

    fn metadata(&self) -> Result<Self::Metadata, Self::Error> {
        Ok(self.metadata.clone())
    }

    fn file_name(&self) -> &std::ffi::OsStr {
        self.path.file_name().expect("Can't get file name")
    }

    fn depth(&self) -> usize {
        0
    }

    fn ino(&self) -> Option<u64> {
        None
    }
}

pub struct InMemoryWalkDir {
    entries: Vec<InMemoryDirEntry>,
}

impl InMemoryWalkDir {
    pub fn new(paths: &[&Path], entries: &[InMemoryDirEntry]) -> Self {
        let entries = entries
            .iter()
            .filter(|e| paths.iter().any(|p| e.path.starts_with(p)))
            .cloned()
            .collect::<Vec<_>>();

        Self { entries }
    }
}

impl IntoIterator for InMemoryWalkDir {
    type Item = Result<InMemoryDirEntry, InMemoryDirWalkerError>;

    type IntoIter = InMemoryWalkDirIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        InMemoryWalkDirIntoIter {
            iter: self.entries.into_iter(),
        }
    }
}

pub struct InMemoryWalkDirIntoIter {
    iter: std::vec::IntoIter<InMemoryDirEntry>,
}

impl Iterator for InMemoryWalkDirIntoIter {
    type Item = Result<InMemoryDirEntry, InMemoryDirWalkerError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(Ok)
    }
}
