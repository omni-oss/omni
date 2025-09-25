use omni_types::OmniPath;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct CacheInfo {
    pub cache_execution: bool,
    pub key_defaults: bool,
    pub key_env_keys: Vec<String>,
    pub key_input_files: Vec<OmniPath>,
    pub cache_output_files: Vec<OmniPath>,
    pub cache_logs: bool,
}
