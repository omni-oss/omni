use std::path::{Path, PathBuf};

use crate::validators::*;
use garde::Validate;
use maps::Map;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::{FsRead, FsReadAsync};

use crate::{
    GeneratorSourceConfiguration, Ui,
    constants::WORKSPACE_NAME_REGEX,
    utils::{self, fs::LoadConfigError},
};

/// # Workspace Configuration
/// This is the configuration file for a workspace.
/// It is used to configure the workspace and its projects.
#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Validate,
)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct WorkspaceConfiguration {
    #[garde(pattern(*WORKSPACE_NAME_REGEX))]
    pub name: Option<String>,

    pub projects: Vec<String>,

    #[serde(default)]
    pub ui: Ui,

    #[serde(default, deserialize_with = "validate_generator_sources")]
    pub generators: Vec<GeneratorSourceConfiguration>,

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
#[serde(deny_unknown_fields)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_configuration_deserializes_valid() {
        let result = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "env": {"vars": {"A": "b"}}}"#,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_workspace_configuration_rejects_unknown_field() {
        let result = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "nope": 1}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_workspace_env_rejects_unknown_field() {
        let result = serde_json::from_str::<WorkspaceEnvConfiguration>(
            r#"{"files": [], "bogus": true}"#,
        );
        assert!(result.is_err());
    }
}
