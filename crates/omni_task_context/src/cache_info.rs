use maps::Map;
use omni_configurations::CacheEnabledSource;
use omni_glob::GlobPatterns;
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheInfo {
    pub cache_enabled: bool,
    /// The tier that decided `cache_enabled`. Provenance only: nothing derives a
    /// cache key from it, so identical eligible tasks hash the same whatever
    /// decided their enablement.
    #[serde(default)]
    pub cache_enabled_source: CacheEnabledSource,
    /// `false` keeps the task off the remote cache (read and write).
    #[serde(default = "default_remote")]
    pub cache_remote: bool,
    pub key_defaults: bool,
    pub key_env_keys: Vec<String>,
    pub key_input_files: GlobPatterns<OmniPath>,
    pub cache_output_files: GlobPatterns<OmniPath>,
    pub cache_logs: bool,
    pub args: Map<String, serde_json::Value>,
    /// The configured `env.vars` definitions, raw and before expansion, kept
    /// as separate layers. Part of the effective task, not of the key's env leaf
    /// (which records the values of `cache.key.env` only). Never loaded values:
    /// `.env` files and the process environment are not here.
    #[serde(default)]
    pub env: EnvLayers,
}

fn default_remote() -> bool {
    true
}

/// The `env.vars` definitions of the three layers, each raw and unmerged.
///
/// They are not merged because a project variable can read a key the workspace
/// also defines and then sees the project's own value of it: expansion starts
/// from the raw definitions, and the workspace values overwrite colliding keys
/// only afterwards. Any definition the task can observe through expansion must
/// therefore reach the key, including a project value the workspace shadows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvLayers {
    pub workspace: Map<String, String>,
    /// The project's definitions, composed through `extends`.
    pub project: Map<String, String>,
    /// The task's own definitions, composed through `extends`.
    pub task: Map<String, String>,
}
