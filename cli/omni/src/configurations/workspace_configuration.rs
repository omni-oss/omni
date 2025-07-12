use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsRead;

use crate::{configurations::ScriptingConfiguration, utils};

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceConfiguration {
    pub name: Option<String>,
    pub projects: Vec<String>,
    #[serde(default)]
    pub env: WorkspaceEnvConfiguration,
    #[serde(default)]
    pub scripting: ScriptingConfiguration,
}

impl WorkspaceConfiguration {
    pub fn load<'a>(
        path: impl Into<&'a Path>,
        sys: impl FsRead,
    ) -> eyre::Result<Self> {
        utils::fs::load_config(path, sys)
    }
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Default,
)]
pub struct WorkspaceEnvConfiguration {
    #[serde(default)]
    pub files: Vec<String>,
}
