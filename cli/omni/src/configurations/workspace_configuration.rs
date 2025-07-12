use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsRead;

use crate::utils;

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceConfiguration {
    pub name: Option<String>,
    pub projects: Vec<String>,
    pub env: Option<WorkspaceEnvConfiguration>,
}

impl WorkspaceConfiguration {
    pub fn load<'a>(
        path: impl Into<&'a Path>,
        fs: impl FsRead,
    ) -> eyre::Result<Self> {
        utils::fs::load_config(path, fs)
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceEnvConfiguration {
    pub files: Vec<String>,
}
