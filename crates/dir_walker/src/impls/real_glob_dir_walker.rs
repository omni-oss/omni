use std::path::{Path, PathBuf};

pub use globset::Error as GlobsetError;
pub use ignore::Error as IgnoreError;
use path_slash::PathExt;

use crate::{
    DirWalkerBase,
    impls::{
        IgnoreOverridesConfig, IgnoreRealDirEntry, IgnoreRealDirWalker,
        IgnoreRealDirWalkerConfig, IgnoreRealDirWalkerError,
        IgnoreRealWalkDirIntoIter, ignore_real_dir_walker,
        ignore_real_dir_walker::Predicate,
    },
};

#[derive(bon::Builder, Default)]
pub struct RealGlobDirWalkerConfig {
    #[builder(into)]
    pub standard_filters: Option<bool>,
    #[builder(into)]
    pub hidden: Option<bool>,
    #[builder(into)]
    pub ignore: Option<bool>,
    #[builder(into)]
    pub git_ignore: Option<bool>,
    #[builder(into)]
    pub git_exclude: Option<bool>,
    #[builder(into)]
    pub git_global: Option<bool>,
    #[builder(into)]
    pub ignore_case_insensitive: Option<bool>,
    #[builder(into, default)]
    pub custom_ignore_filenames: Vec<String>,
    #[builder(into, default)]
    pub include: Vec<PathBuf>,
    #[builder(into, default)]
    pub exclude: Vec<PathBuf>,
    #[builder(into)]
    pub root_dir: PathBuf,
    #[builder(into)]
    pub follow_links: Option<bool>,
    #[builder(setters(vis = "", name = filter_entry_internal))]
    pub filter_entry: Option<Predicate>,
}

impl<S: real_glob_dir_walker_config_builder::State>
    RealGlobDirWalkerConfigBuilder<S>
{
    pub fn filter_entry<F>(
        self,
        filter_entry: F,
    ) -> RealGlobDirWalkerConfigBuilder<
        real_glob_dir_walker_config_builder::SetFilterEntry<S>,
    >
    where
        F: Fn(&ignore::DirEntry) -> bool + Send + Sync + 'static,
        S::FilterEntry: real_glob_dir_walker_config_builder::IsUnset,
    {
        self.filter_entry_internal(std::sync::Arc::new(filter_entry))
    }
}

impl RealGlobDirWalkerConfig {
    fn build_base(
        &self,
    ) -> Result<IgnoreRealDirWalker, IgnoreRealDirWalkerError> {
        let dir_walker =
            IgnoreRealDirWalker::new_with_config(IgnoreRealDirWalkerConfig {
                standard_filters: self.standard_filters,
                custom_ignore_filenames: self.custom_ignore_filenames.clone(),
                follow_links: self.follow_links,
                filter_entry: self.filter_entry.clone(),
                git_exclude: self.git_exclude,
                git_global: self.git_global,
                ignore_case_insensitive: self.ignore_case_insensitive,
                hidden: self.hidden,
                git_ignore: self.git_ignore,
                ignore: self.ignore,
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
        RealGlobDirWalkerConfig::builder()
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

#[cfg(all(test, unix))]
mod tests {
    use std::collections::BTreeSet;

    use crate::{DirEntry as _, DirWalker as _};

    use super::*;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, content).expect("write file");
    }

    fn walked_rel_paths(
        root: &Path,
        follow_links: Option<bool>,
        filter_entry: Option<Predicate>,
    ) -> BTreeSet<PathBuf> {
        let config = RealGlobDirWalkerConfig {
            standard_filters: Some(false),
            root_dir: root.to_path_buf(),
            follow_links,
            filter_entry,
            ..Default::default()
        };
        let walker = config.build_walker().expect("build walker");

        walker
            .walk_dir(&[root])
            .expect("walk")
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                entry
                    .path()
                    .strip_prefix(root)
                    .ok()
                    .map(|p| p.to_path_buf())
            })
            .filter(|p| !p.as_os_str().is_empty())
            .collect()
    }

    #[test]
    fn follow_links_descends_symlinked_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("real/inner.txt"), "x");
        std::os::unix::fs::symlink(root.join("real"), root.join("link"))
            .expect("symlink");

        let followed = walked_rel_paths(root, Some(true), None);
        assert!(
            followed.contains(&PathBuf::from("link/inner.txt")),
            "following should descend the symlinked directory: {followed:?}",
        );
    }

    #[test]
    fn default_does_not_descend_symlinked_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("real/inner.txt"), "x");
        std::os::unix::fs::symlink(root.join("real"), root.join("link"))
            .expect("symlink");

        let default = walked_rel_paths(root, None, None);
        assert!(
            !default.contains(&PathBuf::from("link/inner.txt")),
            "default must not descend the symlinked directory: {default:?}",
        );

        let off = walked_rel_paths(root, Some(false), None);
        assert!(
            !off.contains(&PathBuf::from("link/inner.txt")),
            "follow_links(false) must not descend: {off:?}",
        );
    }

    #[test]
    fn filter_entry_blocks_yield_and_descent() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();

        write_file(&root.join("real/inner.txt"), "x");
        std::os::unix::fs::symlink(root.join("real"), root.join("link"))
            .expect("symlink");

        let predicate: Predicate =
            std::sync::Arc::new(|entry: &ignore::DirEntry| {
                !entry.path_is_symlink()
            });

        let walked = walked_rel_paths(root, Some(true), Some(predicate));

        assert!(
            !walked.contains(&PathBuf::from("link")),
            "filtered symlink must not be yielded: {walked:?}",
        );
        assert!(
            !walked.contains(&PathBuf::from("link/inner.txt")),
            "filtered symlink must not be descended: {walked:?}",
        );
        assert!(
            walked.contains(&PathBuf::from("real/inner.txt")),
            "non-symlinked content must still be yielded: {walked:?}",
        );
    }
}
