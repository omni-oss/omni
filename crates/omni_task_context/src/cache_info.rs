use maps::Map;
use omni_glob::GlobPatterns;
use omni_types::OmniPath;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheInfo {
    pub cache_enabled: bool,
    pub key_defaults: bool,
    pub key_env_keys: Vec<String>,
    pub key_input_files: GlobPatterns<OmniPath>,
    pub cache_output_files: GlobPatterns<OmniPath>,
    pub cache_logs: bool,
    pub args: Map<String, serde_json::Value>,
}
