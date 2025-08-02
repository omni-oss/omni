use std::path::{Path, PathBuf};

use config_utils::{DictConfig, ListConfig, Replace, merge::Merge};
use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsRead;

use crate::{
    configurations::{ExtensionGraphNode, utils::list_config_default},
    utils,
};

use super::TaskConfiguration;

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Merge,
    Validate,
)]
#[garde(allow_unvalidated)]
pub struct ProjectConfiguration {
    #[serde(default, skip)]
    #[merge(skip)]
    pub path: String,

    #[serde(default)]
    #[merge(skip)]
    pub extends: Vec<String>,

    #[merge(strategy = config_utils::replace)]
    pub name: String,

    #[serde(default)]
    pub tasks: DictConfig<TaskConfiguration>,

    #[serde(default = "list_config_default::<Replace<String>>")]
    pub dependencies: ListConfig<Replace<String>>,

    #[serde(default)]
    pub env: ProjectEnvConfiguration,
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
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Eq,
    Merge,
    Default,
    Validate,
)]
#[garde(allow_unvalidated)]
pub struct ProjectEnvConfiguration {
    #[serde(default = "super::utils::list_config_default::<Replace<PathBuf>>")]
    pub files: ListConfig<Replace<PathBuf>>,

    #[serde(default)]
    pub vars: DictConfig<Replace<String>>,
}
