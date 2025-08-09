use std::path::{Path, PathBuf};

use derive_builder::Builder;
pub use globset::Error as GlobsetError;
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
    #[builder(setter(into), default)]
    include: Vec<PathBuf>,
    #[builder(setter(into), default)]
    exclude: Vec<PathBuf>,
}

impl RealGlobDirWalker {
    pub fn builder() -> RealGlobDirWalkerBuilder {
        RealGlobDirWalkerBuilder::default()
    }
}

impl RealGlobDirWalkerBuilder {}

fn pathbuf_to_glob(base_dir: &Path, path: &PathBuf) -> globset::Glob {
    let p = std::path::absolute(base_dir.join(path))
        .expect("failed to resolve path");
    let str = p.to_string_lossy();

    globset::Glob::new(&str).expect("failed to create glob")
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

        let mut include_globset = globset::GlobSetBuilder::new();

        for p in &self.include {
            include_globset.add(pathbuf_to_glob(path, p));
        }

        let mut exclude_globset = globset::GlobSetBuilder::new();

        for p in &self.exclude {
            exclude_globset.add(pathbuf_to_glob(path, p));
        }

        RealGlobDirWalkDir {
            base: dir_walker.base_walk_dir(path).into_iter(),
            include_globset: include_globset
                .build()
                .expect("failed to build globset"),
            exclude_globset: exclude_globset
                .build()
                .expect("failed to build globset"),
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
