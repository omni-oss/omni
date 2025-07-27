use std::path::Path;

use config_utils::{DictConfig, ListConfig, Replace, merge::Merge};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsRead;

use crate::{configurations::ExtensionGraphNode, utils};

use super::TaskConfiguration;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Eq, Merge,
)]
pub struct ProjectConfiguration {
    #[serde(default, skip)]
    #[merge(skip)]
    pub path: String,

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

impl ExtensionGraphNode for ProjectConfiguration {
    type Id = String;

    fn id(&self) -> &Self::Id {
        &self.path
    }

    fn set_id(&mut self, id: &Self::Id) {
        self.path = id.clone();
    }

    fn extendee_ids(&self) -> &[Self::Id] {
        &self.extends
    }

    fn set_extendee_ids(&mut self, extendee_ids: &[Self::Id]) {
        self.extends = extendee_ids.to_vec();
    }
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
