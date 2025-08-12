use std::path::{Path, PathBuf};

use config_utils::{DictConfig, ListConfig, Replace, merge::Merge};
use garde::Validate;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsRead;

use crate::{
    configurations::{
        CacheConfiguration, ExtensionGraphNode, utils::list_config_default,
    },
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
    pub file: OmniPath,

    #[serde(default, skip)]
    pub dir: OmniPath,

    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub base: bool,

    #[serde(default)]
    #[merge(strategy = config_utils::replace)]
    pub extends: Vec<OmniPath>,

    #[merge(strategy = config_utils::replace)]
    pub name: String,

    #[serde(default)]
    pub tasks: DictConfig<TaskConfiguration>,

    #[serde(default = "list_config_default::<Replace<String>>")]
    pub dependencies: ListConfig<Replace<String>>,

    #[serde(default)]
    pub env: ProjectEnvConfiguration,

    #[serde(default)]
    pub cache: CacheConfiguration,
}

impl ExtensionGraphNode for ProjectConfiguration {
    type Id = OmniPath;

    fn id(&self) -> &Self::Id {
        &self.file
    }

    fn extendee_ids(&self) -> &[Self::Id] {
        &self.extends
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
