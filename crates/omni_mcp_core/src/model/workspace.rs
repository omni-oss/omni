use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceInfoResult {
    pub root_dir: String,
    pub cache_dir: String,
    pub env_vars: BTreeMap<String, String>,
}
