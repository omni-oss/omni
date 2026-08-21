use std::path::{Path, PathBuf};

use crate::capabilities::Workspace;
use crate::validators::*;
use garde::Validate;
use maps::Map;
use omni_capabilities::CapabilityPolicyConfig;
pub use omni_experimental_features::ExperimentalFeatures;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::{FsRead, FsReadAsync};

use crate::{
    GeneratorSourceConfiguration, ToolSourceConfiguration, Ui,
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

    /// Registered sources of tool manifests, mirroring `generators`. Supports
    /// `local` and `git` sources; discovery globs each source for
    /// `tool.omni.{yaml,yml,json,toml}` manifests.
    #[serde(default, deserialize_with = "validate_tool_sources")]
    pub tools: Vec<ToolSourceConfiguration>,

    #[serde(default)]
    pub env: WorkspaceEnvConfiguration,

    /// Opts this workspace into experimental / in-progress features.
    ///
    /// Accepts either a bare boolean (enable or disable *every* experimental
    /// feature) or a per-feature map that toggles features by name:
    ///
    /// ```yaml
    /// enable_experimental: true          # all experimental features
    /// # or
    /// enable_experimental:
    ///   capabilities: true               # just the named feature(s)
    /// ```
    ///
    /// Off by default. Currently the only feature is **capabilities**
    /// (capability-based sandboxing and enforcement of generator scripts): when
    /// disabled, declared capabilities are still parsed and validated but not
    /// enforced, and generator scripts run unconfined; when enabled, the
    /// declared capability policy is enforced.
    #[serde(default)]
    pub enable_experimental: ExperimentalFeatures,

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
    #[allow(clippy::result_large_err)]
    pub async fn load_async<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config_async(path, sys).await
    }

    #[allow(clippy::result_large_err)]
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
    fn test_enable_experimental_defaults_to_disabled() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": []}"#,
        )
        .expect("valid");
        assert!(!cfg.enable_experimental.capabilities());
    }

    #[test]
    fn test_enable_experimental_bool_form_toggles_every_feature() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "enable_experimental": true}"#,
        )
        .expect("valid");
        assert!(cfg.enable_experimental.capabilities());
        assert!(cfg.enable_experimental.is_enabled("anything-else"));
    }

    #[test]
    fn test_enable_experimental_per_feature_form() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "enable_experimental": {"capabilities": true}}"#,
        )
        .expect("valid");
        assert!(cfg.enable_experimental.capabilities());
        assert!(!cfg.enable_experimental.is_enabled("other"));
    }

    #[test]
    fn test_enable_experimental_per_feature_false_disables() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "enable_experimental": {"capabilities": false}}"#,
        )
        .expect("valid");
        assert!(!cfg.enable_experimental.capabilities());
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

    #[test]
    fn test_tools_sources_parse_local_and_git() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "tools": [{"source": "local", "path": "./tools"}, {"source": "git", "uri": "https://example.com/a.git", "rev": "main"}]}"#,
        )
        .expect("valid tools sources");
        assert_eq!(cfg.tools.len(), 2);
    }

    #[test]
    fn test_tools_defaults_to_empty() {
        let cfg = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": []}"#,
        )
        .expect("valid");
        assert!(cfg.tools.is_empty());
    }

    #[test]
    fn test_tools_rejects_duplicate_git_uri() {
        let result = serde_json::from_str::<WorkspaceConfiguration>(
            r#"{"projects": [], "tools": [{"source": "git", "uri": "https://example.com/a.git", "rev": "main"}, {"source": "git", "uri": "https://example.com/a.git", "rev": "dev"}]}"#,
        );
        assert!(result.is_err(), "duplicate git uri must be rejected");
    }
}
