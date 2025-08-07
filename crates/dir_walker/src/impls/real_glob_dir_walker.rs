use derive_builder::Builder;
pub use globset::Error as GlobsetError;
use globset::Glob;
pub use ignore::Error as IgnoreError;

use crate::{
    DirEntry, DirWalkerBase,
    impls::{
        IgnoreRealDirEntry, IgnoreRealDirWalker, IgnoreRealDirWalkerConfig,
        IgnoreRealWalkDirIntoIter,
    },
};

#[derive(Builder)]
#[builder(setter(into, strip_option), name = "RealGlobDirWalkerBuilder")]
#[derive(Default)]
pub struct RealGlobDirWalker {
    #[builder(default = "true")]
    standard_filters: bool,
    #[builder(default)]
    custom_ignore_filenames: Vec<String>,
    #[builder(setter(custom), default)]
    include: globset::GlobSet,
    #[builder(setter(custom), default)]
    exclude: globset::GlobSet,
}

impl RealGlobDirWalker {
    pub fn builder() -> RealGlobDirWalkerBuilder {
        RealGlobDirWalkerBuilder::default()
    }
}

impl RealGlobDirWalkerBuilder {
    pub fn include<'s, 'a>(
        &'s mut self,
        globs: impl AsRef<[&'a str]>,
    ) -> Result<&'s mut Self, globset::Error> {
        let mut set = globset::GlobSetBuilder::new();

        for glob in globs.as_ref() {
            set.add(Glob::new(glob)?);
        }

        self.include = Some(set.build()?);

        Ok(self)
    }

    pub fn exclude<'s, 'a>(
        &'s mut self,
        globs: impl AsRef<[&'a str]>,
    ) -> Result<&'s mut Self, globset::Error> {
        let mut set = globset::GlobSetBuilder::new();

        for glob in globs.as_ref() {
            set.add(Glob::new(glob)?);
        }

        self.exclude = Some(set.build()?);

        Ok(self)
    }
}

impl DirWalkerBase for RealGlobDirWalker {
    type DirEntry = IgnoreRealDirEntry;

    type Error = ignore::Error;

    type WalkDir = RealGlobDirWalkDir;

    fn base_walk_dir(&self, path: &std::path::Path) -> Self::WalkDir {
        let dir_walker =
            IgnoreRealDirWalker::new_with_config(IgnoreRealDirWalkerConfig {
                standard_filters: self.standard_filters,
                custom_ignore_filenames: self.custom_ignore_filenames.clone(),
            });

        RealGlobDirWalkDir {
            base: dir_walker.base_walk_dir(path).into_iter(),
            include_globset: self.include.clone(),
            exclude_globset: self.exclude.clone(),
        }
    }
}

pub struct RealGlobDirWalkDir {
    base: IgnoreRealWalkDirIntoIter,
    include_globset: globset::GlobSet,
    exclude_globset: globset::GlobSet,
}

impl Iterator for RealGlobDirWalkDir {
    type Item = Result<IgnoreRealDirEntry, ignore::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        for result in self.base.by_ref() {
            match result {
                Ok(r) => {
                    let path = r.path().to_string_lossy();

                    if self.exclude_globset.is_match(&*path)
                        || !self.include_globset.is_match(&*path)
                    {
                        continue;
                    }

                    return Some(Ok(r));
                }
                r => return Some(r),
            }
        }

        None
    }
}
