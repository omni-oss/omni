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
}

fn default_remote() -> bool {
    true
}
