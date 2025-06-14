use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::utils;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceConfiguration {
    pub name: Option<String>,
    pub projects: Vec<String>,
    pub env: Option<WorkspaceEnvConfiguration>,
}

impl WorkspaceConfiguration {
    pub fn load<'a>(path: impl Into<&'a Path>) -> eyre::Result<Self> {
        utils::fs::load_config(path)
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceEnvConfiguration {
    pub files: Vec<String>,
}
