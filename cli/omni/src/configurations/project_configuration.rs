use std::{collections::HashMap, path::Path};

use config_utils::ListConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsRead;

use crate::utils;

use super::{DependencyConfiguration, TaskConfiguration};

fn default_true() -> bool {
    false
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct ProjectConfiguration {
    pub name: String,
    #[serde(default = "default_true")]
    pub implicit_tasks: bool,
    pub tasks: Option<HashMap<String, TaskConfiguration>>,
    #[serde(default)]
    pub dependencies: Vec<DependencyConfiguration>,
    #[serde(default = "default_true")]
    pub implicit_dependencies: bool,
    pub env: Option<ProjectEnvConfiguration>,
}

impl ProjectConfiguration {
    pub fn load<'a>(
        path: impl Into<&'a Path>,
        sys: impl FsRead,
    ) -> eyre::Result<Self> {
        utils::fs::load_config(path, sys)
    }
}

#[derive(Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq)]
pub struct ProjectEnvConfiguration {
    pub files: Option<ListConfig<String>>,
}
