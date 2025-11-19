use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use derive_new::new;
use dir_walker::{
    DirEntry as _, DirWalker, Metadata as _,
    impls::{IgnoreRealDirWalker, IgnoreRealDirWalkerConfig},
};
use omni_discovery_utils::glob::GlobMatcher;

use crate::error::{Error, ErrorInner};

#[derive(Debug, Clone, new)]
pub struct ConfigurationDiscovery<'a, G, C, I>
where
    G: AsRef<str>,
    C: AsRef<str>,
    I: AsRef<str>,
{
    #[new(into)]
    root_dir: &'a Path,

    #[new(into)]
    glob_patterns: &'a [G],

    #[new(into)]
    config_files: &'a [C],

    #[new(into)]
    ignore_files: &'a [I],

    #[new(into)]
    config_name: &'a str,
}

impl<'a, G, C, I> ConfigurationDiscovery<'a, G, C, I>
where
    G: AsRef<str>,
    C: AsRef<str>,
    I: AsRef<str>,
{
    fn create_default_dir_walker(
        &self,
    ) -> Result<impl DirWalker + 'static, Error> {
        let mut cfg_builder = IgnoreRealDirWalkerConfig::builder();

        let cfg = cfg_builder
            .standard_filters(true)
            .custom_ignore_filenames(
                self.ignore_files
                    .iter()
                    .map(|s| s.as_ref().to_string())
                    .collect::<Vec<_>>(),
            )
            .build()?;

        Ok(IgnoreRealDirWalker::new_with_config(cfg))
    }

    pub async fn discover(&self) -> Result<Vec<PathBuf>, Error> {
        let walker = self.create_default_dir_walker()?;
        self.discover_with_walker(&walker).await
    }

    pub async fn discover_with_walker<TDirWalker: DirWalker>(
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
            trace::trace!("checking path: {:?}", f.path());

            let meta = f.metadata().map_err(|e| {
                ErrorInner::new_failed_to_get_metadata(
                    f.path().to_path_buf(),
                    e,
                )
            })?;

            if meta.is_dir() {
                continue;
            }

            if matcher.is_match(f.path()) {
                for file_name in self.config_files {
                    if *f.file_name().to_string_lossy() == *file_name.as_ref() {
                        trace::trace!(
                            "Found {} config: {:?}",
                            self.config_name,
                            f.path()
                        );
                        discovered.push(f.path().to_path_buf());
                        break;
                    }
                }
            }
        }

        trace::debug!(
            "Found {} {} configs in {:?}, walked {} items",
            discovered.len(),
            self.config_name,
            start_walk_time.elapsed().unwrap_or_default(),
            num_iterations
        );

        Ok(discovered)
    }
}
