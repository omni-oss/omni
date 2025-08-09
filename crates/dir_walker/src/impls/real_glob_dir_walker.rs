use std::path::{Path, PathBuf};

use derive_builder::Builder;
pub use globset::Error as GlobsetError;
pub use ignore::Error as IgnoreError;

use crate::{
    DirEntry, DirWalkerBase,
    impls::{
        IgnoreRealDirEntry, IgnoreRealDirWalker, IgnoreRealDirWalkerConfig,
        IgnoreRealWalkDirIntoIter, ignore_real_dir_walker,
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

fn pathbuf_to_glob(path: &Path) -> globset::Glob {
    let str = path.to_string_lossy();

    globset::Glob::new(&str).expect("failed to create glob")
}

fn join_abs(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
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
        let dir_walker =
            IgnoreRealDirWalker::new_with_config(IgnoreRealDirWalkerConfig {
                standard_filters: self.standard_filters,
                custom_ignore_filenames: self.custom_ignore_filenames.clone(),
            });

        let mut include_globset = globset::GlobSetBuilder::new();

        for p in &self.include {
            if p.is_absolute() {
                include_globset.add(pathbuf_to_glob(p));
            } else {
                for base in paths {
                    let p = join_abs(base, p);
                    include_globset.add(pathbuf_to_glob(&p));
                }
            }
        }

        let mut exclude_globset = globset::GlobSetBuilder::new();

        for p in &self.exclude {
            if p.is_absolute() {
                exclude_globset.add(pathbuf_to_glob(p));
            } else {
                for base in paths {
                    let p = join_abs(base, p);
                    exclude_globset.add(pathbuf_to_glob(&p));
                }
            }
        }

        Ok(RealGlobDirWalkDir {
            base: dir_walker.base_walk_dir(paths)?.into_iter(),
            include_globset: include_globset
                .build()
                .expect("failed to build globset"),
            exclude_globset: exclude_globset
                .build()
                .expect("failed to build globset"),
        })
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
