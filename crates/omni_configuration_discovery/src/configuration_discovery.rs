use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use derive_new::new;
use dir_walker::{
    DirEntry as _, DirWalker, Metadata as _,
    impls::{IgnoreRealDirWalker, IgnoreRealDirWalkerConfig},
};
use globset::{Glob, GlobSetBuilder};

use crate::error::{Error, ErrorInner};

#[derive(Debug, Clone, new)]
pub struct ConfigurationDiscovery<'a> {
    #[new(into)]
    root_dir: &'a Path,

    #[new(into)]
    project_patterns: &'a [String],

    #[new(into)]
    config_file_names: &'a [String],

    #[new(into)]
    ignore_filenames: &'a [String],

    #[new(into)]
    config_name: &'a str,
}

impl<'a> ConfigurationDiscovery<'a> {
    fn create_default_dir_walker(
        &self,
    ) -> Result<impl DirWalker + 'static, Error> {
        let mut cfg_builder = IgnoreRealDirWalkerConfig::builder();

        let mut globset = GlobSetBuilder::new();

        let root = self.root_dir.to_string_lossy().to_string();
        let root = if cfg!(windows) && root.contains('\\') {
            root.replace('\\', "/")
        } else {
            root
        };

        for glob in self.project_patterns {
            globset.add(
                Glob::new(&format!("{}/{}", root, glob))
                    .expect("can't create glob"),
            );
        }
        let matcher = globset.build()?;

        let cfg = cfg_builder
            .standard_filters(true)
            .filter_entry(move |entry| matcher.is_match(entry.path()))
            .custom_ignore_filenames(self.ignore_filenames.to_vec())
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

        let mut match_b = GlobSetBuilder::new();

        for p in self.project_patterns {
            match_b.add(Glob::new(
                format!("{}/{}", self.root_dir.display(), p).as_str(),
            )?);
        }

        let matcher = match_b.build()?;

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
                for file_name in self.config_file_names {
                    if *f.file_name().to_string_lossy() == *file_name {
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
