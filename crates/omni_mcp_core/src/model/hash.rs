use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HashProjectParams {
    pub name: String,
    #[serde(default)]
    pub tasks: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HashResult {
    pub hash: String,
}
