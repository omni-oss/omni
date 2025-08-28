use std::path::{Path, PathBuf};

use derive_builder::Builder;
pub use globset::Error as GlobsetError;
pub use ignore::Error as IgnoreError;
use path_slash::PathExt;

use crate::{
    DirWalkerBase,
    impls::{
        IgnoreOverridesConfig, IgnoreRealDirEntry, IgnoreRealDirWalker,
        IgnoreRealDirWalkerConfig, IgnoreRealDirWalkerError,
        IgnoreRealWalkDirIntoIter, ignore_real_dir_walker,
    },
};

#[derive(Builder)]
#[builder(setter(into, strip_option), name = "RealGlobDirWalkerConfigBuilder")]
#[derive(Default)]
pub struct RealGlobDirWalkerConfig {
    #[builder(default = "true")]
    standard_filters: bool,

    #[builder(default)]
    custom_ignore_filenames: Vec<String>,

    #[builder(setter(into), default)]
    include: Vec<PathBuf>,

    #[builder(setter(into), default)]
    exclude: Vec<PathBuf>,

    #[builder(setter(into))]
    root_dir: PathBuf,
}

impl RealGlobDirWalkerConfig {
    pub fn builder() -> RealGlobDirWalkerConfigBuilder {
        RealGlobDirWalkerConfigBuilder::default()
    }

    fn build_base(
        &self,
    ) -> Result<IgnoreRealDirWalker, IgnoreRealDirWalkerError> {
        let dir_walker =
            IgnoreRealDirWalker::new_with_config(IgnoreRealDirWalkerConfig {
                standard_filters: self.standard_filters,
                custom_ignore_filenames: self.custom_ignore_filenames.clone(),
                filter_entry: None,
                overrides: Some(IgnoreOverridesConfig {
                    root: self.root_dir.to_string_lossy().to_string(),
                    excludes: self
                        .exclude
                        .iter()
                        .map(|p| {
                            try_relpath(&self.root_dir, p)
                                .to_slash_lossy()
                                .to_string()
                        })
                        .collect(),
                    includes: self
                        .include
                        .iter()
                        .map(|p| {
                            try_relpath(&self.root_dir, p)
                                .to_slash_lossy()
                                .to_string()
                        })
                        .collect(),
                }),
            });

        Ok(dir_walker)
    }

    pub fn build_walker(
        self,
    ) -> Result<RealGlobDirWalker, IgnoreRealDirWalkerError> {
        RealGlobDirWalker::new(self)
    }
}

#[derive(Default)]
pub struct RealGlobDirWalker {
    base: IgnoreRealDirWalker,
}

impl RealGlobDirWalker {
    pub fn config() -> RealGlobDirWalkerConfigBuilder {
        RealGlobDirWalkerConfigBuilder::default()
    }

    pub fn new(
        config: RealGlobDirWalkerConfig,
    ) -> Result<Self, IgnoreRealDirWalkerError> {
        let dir_walker = config.build_base()?;

        Ok(Self { base: dir_walker })
    }
}

fn try_relpath<'a>(base: &Path, path: &'a Path) -> &'a Path {
    if path.starts_with(base) {
        path.strip_prefix(base).unwrap()
    } else {
        path
    }
}

impl DirWalkerBase for RealGlobDirWalker {
    type DirEntry = IgnoreRealDirEntry;
    type Error = ignore_real_dir_walker::IgnoreRealDirWalkerError;
    type IterError = ignore::Error;
    type WalkDir = RealGlobDirWalkDir;

    fn base_walk_dir(
        &self,
        paths: &[&std::path::Path],
    ) -> Result<Self::WalkDir, Self::Error> {
        Ok(RealGlobDirWalkDir {
            base: self.base.base_walk_dir(paths)?.into_iter(),
        })
    }
}

pub struct RealGlobDirWalkDir {
    base: IgnoreRealWalkDirIntoIter,
}

impl Iterator for RealGlobDirWalkDir {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.base.next()
    }
}
