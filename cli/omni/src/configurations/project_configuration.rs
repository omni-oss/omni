use std::path::Path;

use config_utils::{DictConfig, ListConfig, Replace, merge::Merge};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsRead;

use crate::utils;

use super::TaskConfiguration;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Merge,
)]
pub struct ProjectConfiguration {
    #[serde(default)]
    #[merge(skip)]
    pub extends: Vec<String>,

    #[merge(strategy = config_utils::replace)]
    pub name: String,

    #[merge(strategy = merge::option::recurse)]
    pub tasks: Option<DictConfig<TaskConfiguration>>,

    #[serde(default)]
    pub dependencies: ListConfig<Replace<String>>,

    #[merge(strategy = merge::option::recurse)]
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

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Merge,
)]
pub struct ProjectEnvConfiguration {
    #[merge(strategy = merge::option::recurse)]
    pub files: Option<ListConfig<Replace<String>>>,
}
