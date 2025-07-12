use crate::{DirEntry, DirWalkerBase};

#[derive(Clone, Debug)]
pub struct RealDirWalkerConfig {
    pub follow_root_links: bool,
    pub follow_links: bool,
}

impl RealDirWalkerConfig {
    pub(crate) fn apply(&self, wd: walkdir::WalkDir) -> walkdir::WalkDir {
        wd.follow_links(self.follow_links)
            .follow_root_links(self.follow_root_links)
    }
}

impl Default for RealDirWalkerConfig {
    fn default() -> Self {
        Self {
            follow_root_links: true,
            follow_links: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RealDirWalker {
    config: RealDirWalkerConfig,
}

impl RealDirWalker {
    pub fn new_with_config(config: RealDirWalkerConfig) -> Self {
        Self { config }
    }

    fn configure(&self, wd: walkdir::WalkDir) -> walkdir::WalkDir {
        self.config.apply(wd)
    }
}

impl DirWalkerBase for RealDirWalker {
    type DirEntry = RealDirEntry;
    type Error = walkdir::Error;
    type WalkDir = RealWalkDir;

    fn base_walk_dir(&self, path: &std::path::Path) -> Self::WalkDir {
        let wd = self.configure(walkdir::WalkDir::new(path));

        RealWalkDir(wd)
    }
}

pub struct RealWalkDir(walkdir::WalkDir);

impl IntoIterator for RealWalkDir {
    type Item = Result<RealDirEntry, walkdir::Error>;

    type IntoIter = RealWalkDirIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        RealWalkDirIntoIter(self.0.into_iter())
    }
}

pub struct RealWalkDirIntoIter(walkdir::IntoIter);

impl Iterator for RealWalkDirIntoIter {
    type Item = Result<RealDirEntry, walkdir::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|e| e.map(RealDirEntry))
    }
}

pub struct RealDirEntry(walkdir::DirEntry);

impl DirEntry for RealDirEntry {
    type Error = walkdir::Error;
    type Metadata = std::fs::Metadata;

    fn path(&self) -> &std::path::Path {
        self.0.path()
    }

    fn into_path(self) -> std::path::PathBuf {
        self.0.into_path()
    }

    fn path_is_symlink(&self) -> bool {
        self.0.path_is_symlink()
    }

    fn metadata(&self) -> Result<Self::Metadata, Self::Error> {
        self.0.metadata()
    }

    fn file_name(&self) -> &std::ffi::OsStr {
        self.0.file_name()
    }

    fn depth(&self) -> usize {
        self.0.depth()
    }

    #[cfg(unix)]
    fn ino(&self) -> Option<u64> {
        use walkdir::DirEntryExt;

        Some(self.0.ino())
    }
}
