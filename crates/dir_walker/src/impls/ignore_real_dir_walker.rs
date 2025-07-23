use crate::{DirEntry, DirWalkerBase};

#[derive(Clone, Debug)]
pub struct IgnoreRealDirWalkerConfig {
    pub standard_filters: bool,
    pub custom_ignore_filenames: Vec<String>,
}

impl Default for IgnoreRealDirWalkerConfig {
    fn default() -> Self {
        Self {
            standard_filters: true,
            custom_ignore_filenames: vec![],
        }
    }
}

impl IgnoreRealDirWalkerConfig {
    pub(crate) fn apply<'builder>(
        &self,
        builder: &'builder mut ignore::WalkBuilder,
    ) -> &'builder mut ignore::WalkBuilder {
        let mut builder = builder.standard_filters(self.standard_filters);

        for ignore_filename in self.custom_ignore_filenames.iter() {
            builder = builder.add_custom_ignore_filename(ignore_filename);
        }

        builder
    }
}

#[derive(Clone, Debug, Default)]
pub struct IgnoreRealDirWalker {
    config: IgnoreRealDirWalkerConfig,
}

impl IgnoreRealDirWalker {
    pub fn new_with_config(config: IgnoreRealDirWalkerConfig) -> Self {
        Self { config }
    }
}

impl DirWalkerBase for IgnoreRealDirWalker {
    type Error = ignore::Error;
    type DirEntry = IgnoreRealDirEntry;
    type WalkDir = IgnoreRealWalkDir;

    fn base_walk_dir(&self, path: &std::path::Path) -> Self::WalkDir {
        let mut builder = ignore::WalkBuilder::new(path);
        let builder = self.config.apply(&mut builder);

        let walk = builder.build();

        IgnoreRealWalkDir { walk }
    }
}

pub struct IgnoreRealDirEntry(ignore::DirEntry);

impl DirEntry for IgnoreRealDirEntry {
    type Metadata = std::fs::Metadata;
    type Error = ignore::Error;

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
        self.0.ino()
    }
}

pub struct IgnoreRealWalkDir {
    walk: ignore::Walk,
}

impl IntoIterator for IgnoreRealWalkDir {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    type IntoIter = IgnoreRealWalkDirIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IgnoreRealWalkDirIntoIter { walk: self.walk }
    }
}

pub struct IgnoreRealWalkDirIntoIter {
    walk: ignore::Walk,
}

impl Iterator for IgnoreRealWalkDirIntoIter {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.walk.next().map(|e| e.map(IgnoreRealDirEntry))
    }
}
