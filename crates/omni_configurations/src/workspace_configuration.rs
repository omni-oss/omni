use std::path::{Path, PathBuf};

use garde::Validate;
use maps::Map;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::{FsRead, FsReadAsync};

use crate::{
    ExecutorsConfiguration, Ui,
    constants::WORKSPACE_NAME_REGEX,
    utils::{self, fs::LoadConfigError},
};

/// # Workspace Configuration
/// This is the configuration file for a workspace.
/// It is used to configure the workspace and its projects.
#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct WorkspaceConfiguration {
    #[garde(pattern(*WORKSPACE_NAME_REGEX))]
    pub name: Option<String>,

    pub projects: Vec<String>,

    #[serde(default)]
    pub ui: Ui,

    #[serde(default)]
    pub executors: ExecutorsConfiguration,

    #[serde(default)]
    pub generators: Vec<String>,

    #[serde(default)]
    pub env: WorkspaceEnvConfiguration,
}

impl WorkspaceConfiguration {
    pub async fn load_async<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config_async(path, sys).await
    }

    pub fn load<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsRead + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config(path, sys)
    }
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct WorkspaceEnvConfiguration {
    #[serde(default = "default_files")]
    pub files: Vec<PathBuf>,

    #[serde(default)]
    pub vars: Map<String, String>,
}

fn default_files() -> Vec<PathBuf> {
    vec![
        PathBuf::from(".env"),
        PathBuf::from(".env.local"),
        PathBuf::from(".env.{ENV}"),
        PathBuf::from(".env.{ENV}.local"),
    ]
}

impl Default for WorkspaceEnvConfiguration {
    fn default() -> Self {
        Self {
            files: default_files(),
            vars: Default::default(),
        }
    }
}
