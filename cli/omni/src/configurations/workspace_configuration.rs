use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceConfiguration {
    pub name: Option<String>,
    pub projects: Vec<String>,
}
