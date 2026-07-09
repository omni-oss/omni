use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use derive_new::new;
use dir_walker::{
    DirEntry as _, DirWalker,
    impls::{IgnoreRealDirWalker, IgnoreRealDirWalkerConfig},
};
use omni_discovery_utils::glob::GlobMatcher;

use crate::error::{Error, ErrorInner};

#[derive(Debug, Clone, new)]
pub struct Discovery<'a> {
    #[new(into)]
    root_dir: &'a Path,

    #[new(into)]
    glob_patterns: &'a [String],

    #[new(into)]
    ignore_files: &'a [String],

    #[new(default)]
    config: Option<DiscoveryConfig>,
}

impl<'a> Discovery<'a> {
    pub fn new_with_config(
        root_dir: impl Into<&'a Path>,
        glob_patterns: impl Into<&'a [String]>,
        ignore_files: impl Into<&'a [String]>,
        config: DiscoveryConfig,
    ) -> Self {
        Self {
            root_dir: root_dir.into(),
            glob_patterns: glob_patterns.into(),
            ignore_files: ignore_files.into(),
            config: Some(config),
        }
    }
}

#[derive(Debug, Clone, Copy, bon::Builder)]
pub struct DiscoveryConfig {
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
}

impl<'a> Discovery<'a> {
    fn create_default_dir_walker(
        &self,
    ) -> Result<impl DirWalker + 'static, Error> {
        let cfg_builder = IgnoreRealDirWalkerConfig::builder();

        let cfg = if let Some(config) = &self.config {
            cfg_builder
                .maybe_standard_filters(config.standard_filters)
                .maybe_hidden(config.hidden)
                .maybe_ignore(config.ignore)
                .maybe_git_ignore(config.git_ignore)
                .maybe_git_exclude(config.git_exclude)
                .maybe_git_global(config.git_global)
                .maybe_ignore_case_insensitive(config.ignore_case_insensitive)
                .custom_ignore_filenames(self.ignore_files.to_vec())
                .build()
        } else {
            cfg_builder
                .standard_filters(true)
                .custom_ignore_filenames(self.ignore_files.to_vec())
                .build()
        };

        Ok(IgnoreRealDirWalker::new_with_config(cfg))
    }

    pub async fn discover(&self) -> Result<Vec<PathBuf>, Error> {
        let walker = self.create_default_dir_walker()?;
        self.discover_with_walker(&walker).await
    }

    pub(crate) async fn discover_with_walker<TDirWalker: DirWalker>(
        &self,
        walker: &TDirWalker,
    ) -> Result<Vec<PathBuf>, Error> {
        let mut discovered = vec![];

        let matcher = GlobMatcher::new(self.root_dir, self.glob_patterns)?;

        let start_walk_time = SystemTime::now();

        let mut num_iterations = 0;

        for f in walker.walk_dir(&[self.root_dir]).map_err(|e| {
            ErrorInner::new_walk_dir(self.root_dir.to_path_buf(), e)
        })? {
            num_iterations += 1;
            let f = f.map_err(ErrorInner::new_failed_to_get_dir_entry)?;
            trace::trace!(path = ?f.path(), "checking_path");

            if f.is_dir() {
                continue;
            }

            if matcher.is_match(f.path()) {
                discovered.push(f.path().to_path_buf());
            }
        }

        log::debug!(
            "Found {} files in {:?}, walked {} items",
            discovered.len(),
            start_walk_time.elapsed().unwrap_or_default(),
            num_iterations
        );

        Ok(discovered)
    }
}
