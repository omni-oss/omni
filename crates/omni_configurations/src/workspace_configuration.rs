use std::path::{Path, PathBuf};

use crate::capabilities::Workspace;
use crate::validators::*;
use garde::Validate;
use maps::Map;
use omni_capabilities::CapabilityPolicyConfig;
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
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
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

    /// Workspace-level capability floor, applied to every run of each
    /// subsystem. `rules` is a single subsystem-tagged list: each rule's
    /// `applies_to.subsystem` selects which subsystem(s) it governs (defaulting
    /// to `all`). Because evaluation is deny-dominant, a workspace `deny` can
    /// never be re-opened by a generator- or action-level `allow`: lower levels
    /// may only narrow this floor. This is what makes confinement mandatory by
    /// default rather than opt-in per generator. `strictness` sets the baseline
    /// floor-gap stance combined most-severe with each generator/action.
    #[serde(default)]
    pub capabilities: CapabilityPolicyConfig<Workspace>,
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

    #[test]
    fn test_capabilities_default_to_empty() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": []}"#,
        )
        .expect("valid");
        assert!(cfg.capabilities.rules.is_empty());
    }

    #[test]
    fn test_subsystem_tagged_capabilities_parse() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "capabilities": {"rules": [{"access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"], "applies_to": {"subsystem": ["generator", "tools"]}}]}}"#,
        )
        .expect("valid");
        assert_eq!(cfg.capabilities.rules.len(), 1);
    }

    #[test]
    fn test_capabilities_reject_unknown_subsystem() {
        // An unknown subsystem name in the tag is rejected.
        let result = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "capabilities": {"rules": [{"access": "allow", "domain": "fs.read", "patterns": ["**"], "applies_to": {"subsystem": ["nope"]}}]}}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_workspace_strictness_parses_and_defaults() {
        use omni_capabilities::CapabilitiesStrictness;

        let default_cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": []}"#,
        )
        .expect("valid");
        assert_eq!(
            default_cfg.capabilities.strictness,
            CapabilitiesStrictness::Warn
        );

        let strict = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "capabilities": {"strictness": "require-floor"}}"#,
        )
        .expect("valid");
        assert_eq!(
            strict.capabilities.strictness,
            CapabilitiesStrictness::RequireFloor
        );
    }
}
