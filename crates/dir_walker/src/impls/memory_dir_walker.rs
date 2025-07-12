use std::path::Path;

use derive_more::Constructor;
use thiserror::Error;

use crate::{DirEntry, DirWalkerBase};

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

    type Error = InMemoryDirWalkerError;

    type WalkDir = InMemoryWalkDir;

    fn base_walk_dir(&self, path: &std::path::Path) -> Self::WalkDir {
        InMemoryWalkDir::new(path, &self.0)
    }
}

#[derive(Clone, Debug, Constructor)]
pub struct InMemoryDirEntry {
    path: std::path::PathBuf,
    is_symlink: bool,
    metadata: InMemoryMetadata,
}

/// Doesn't do anything for now
#[derive(Clone, Debug, Default)]
pub struct InMemoryMetadata;

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
    pub fn new(path: &Path, entries: &[InMemoryDirEntry]) -> Self {
        let entries = entries
            .iter()
            .filter(|e| e.path.starts_with(path))
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
