use std::path::{Path, PathBuf};

use garde::Validate;
use maps::Map;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;

use crate::{
    ExecutorsConfiguration,
    constants::WORKSPACE_NAME_REGEX,
    utils::{self, fs::LoadConfigError},
};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct WorkspaceConfiguration {
    #[garde(pattern(*WORKSPACE_NAME_REGEX))]
    pub name: Option<String>,

    pub projects: Vec<String>,

    #[serde(default)]
    pub executors: ExecutorsConfiguration,

    #[serde(default)]
    pub generators: Vec<String>,

    #[serde(default)]
    pub env: WorkspaceEnvConfiguration,
}

impl WorkspaceConfiguration {
    pub async fn load<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config(path, sys).await
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
