use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use derive_new::new;
use dir_walker::DirWalker;
use omni_configuration_discovery::ConfigurationDiscovery;
use thiserror::Error;

use crate::constants;

#[derive(Debug, Clone)]
pub struct ProjectDiscovery<'a> {
    discovery: ConfigurationDiscovery<'a, String>,
}

static IGNORE_FILES: LazyLock<[String; 1]> =
    LazyLock::new(|| [constants::PROJECT_OMNI.to_string()]);

static CONFIG_FILES: LazyLock<Vec<String>> = LazyLock::new(|| {
    constants::SUPPORTED_EXTENSIONS
        .iter()
        .map(|ext| constants::PROJECT_OMNI.replace("{ext}", ext))
        .collect()
});

impl<'a> ProjectDiscovery<'a> {
    pub fn new(root_dir: &'a Path, project_patterns: &'a [String]) -> Self {
        Self {
            discovery: ConfigurationDiscovery::new(
                root_dir,
                project_patterns,
                &CONFIG_FILES[..],
                &IGNORE_FILES[..],
                "project",
            ),
        }
    }
}

impl<'a> ProjectDiscovery<'a> {
    pub async fn discover_project_files(
        &self,
    ) -> Result<Vec<DiscoveredPath>, ProjectDiscoveryError> {
        let discovered = self.discovery.discover().await?;

        Ok(discovered
            .into_iter()
            .map(|p| DiscoveredPath::new_real(p))
            .collect())
    }

    pub async fn discover_project_files_with_walker<TDirWalker: DirWalker>(
        &self,
        walker: &TDirWalker,
    ) -> Result<Vec<DiscoveredPath>, ProjectDiscoveryError> {
        let discovered = self.discovery.discover_with_walker(walker).await?;

        Ok(discovered
            .into_iter()
            .map(|p| DiscoveredPath::new_real(p))
            .collect())
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
#[error(transparent)]
pub struct ProjectDiscoveryError(
    #[from] pub(crate) omni_configuration_discovery::error::Error,
);
