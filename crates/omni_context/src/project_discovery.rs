use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use derive_new::new;
use dir_walker::{
    DirEntry as _, DirWalker, Metadata as _,
    impls::{
        IgnoreRealDirWalker, IgnoreRealDirWalkerConfig,
        IgnoreRealDirWalkerConfigBuilderError,
    },
};
use globset::{Glob, GlobSetBuilder};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use thiserror::Error;

use crate::constants;

#[derive(Debug, Clone, new)]
pub struct ProjectDiscovery<'a> {
    #[new(into)]
    root_dir: &'a Path,
    #[new(into)]
    project_patterns: &'a [String],
}

impl<'a> ProjectDiscovery<'a> {
    fn create_default_dir_walker(
        &self,
    ) -> Result<impl DirWalker + 'static, ProjectDiscoveryError> {
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
            .custom_ignore_filenames(vec![constants::OMNI_IGNORE.to_string()])
            .build()?;

        Ok(IgnoreRealDirWalker::new_with_config(cfg))
    }

    pub async fn discover_project_files(
        &self,
    ) -> Result<Vec<DiscoveredPath>, ProjectDiscoveryError> {
        let walker = self.create_default_dir_walker()?;
        self.discover_project_files_with_walker(&walker).await
    }

    pub async fn discover_project_files_with_walker<TDirWalker: DirWalker>(
        &self,
        walker: &TDirWalker,
    ) -> Result<Vec<DiscoveredPath>, ProjectDiscoveryError> {
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

        let project_files: Vec<_> = constants::SUPPORTED_EXTENSIONS
            .iter()
            .map(|ext| constants::PROJECT_OMNI.replace("{ext}", ext))
            .collect();

        for f in walker.walk_dir(&[self.root_dir]).map_err(|e| {
            ProjectDiscoveryErrorInner::new_walk_dir(
                self.root_dir.to_path_buf(),
                e,
            )
        })? {
            num_iterations += 1;
            let f = f.map_err(
                ProjectDiscoveryErrorInner::new_failed_to_get_dir_entry,
            )?;
            trace::trace!("checking path: {:?}", f.path());

            let meta = f.metadata().map_err(|e| {
                ProjectDiscoveryErrorInner::new_failed_to_get_metadata(
                    f.path().to_path_buf(),
                    e,
                )
            })?;

            if meta.is_dir() {
                continue;
            }

            if matcher.is_match(f.path()) {
                for project_file in &project_files {
                    if *f.file_name().to_string_lossy() == *project_file {
                        trace::trace!(
                            "Found project directory: {:?}",
                            f.path()
                        );
                        discovered.push(DiscoveredPath::new_real(
                            f.path().to_path_buf(),
                        ));
                        break;
                    }
                }
            }
        }

        trace::debug!(
            "Found {} project directories in {:?}, walked {} items",
            discovered.len(),
            start_walk_time.elapsed().unwrap_or_default(),
            num_iterations
        );

        Ok(discovered)
    }
}

#[derive(Debug, Clone, new)]
pub enum DiscoveredPath {
    Real {
        #[new(into)]
        file: PathBuf,
    },
    #[allow(unused)]
    Virtual {
        #[new(into)]
        dir: PathBuf,
    },
}

#[derive(Error, Debug)]
#[error("{inner}")]
pub struct ProjectDiscoveryError {
    #[source]
    inner: ProjectDiscoveryErrorInner,
    kind: ProjectDiscoveryErrorKind,
}

impl ProjectDiscoveryError {
    #[allow(unused)]
    pub fn kind(&self) -> ProjectDiscoveryErrorKind {
        self.kind
    }
}

impl<T: Into<ProjectDiscoveryErrorInner>> From<T> for ProjectDiscoveryError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Error, Debug, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ProjectDiscoveryErrorKind))]
enum ProjectDiscoveryErrorInner {
    #[error(transparent)]
    Glob(#[from] globset::Error),

    #[error("failed to walk dir: {dir}")]
    WalkDir {
        dir: PathBuf,
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error("failed to get metadata for path: {path}")]
    FailedToGetMetadata {
        path: PathBuf,
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error("failed to get dir entry")]
    FailedToGetDirEntry {
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error(transparent)]
    Unknown(
        #[new(into)]
        #[from]
        eyre::Report,
    ),

    #[error(transparent)]
    IgnoreRealDirWalkerConfigBuilderError(
        #[from] IgnoreRealDirWalkerConfigBuilderError,
    ),
}
