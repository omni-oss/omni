use std::path::Path;

use config_utils::{DictConfig, ListConfig, Replace, merge::Merge};
use garde::Validate;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;

use crate::{
    CacheConfiguration, MetaConfiguration,
    utils::{self, fs::LoadConfigError, list_config_default},
};

use super::TaskConfiguration;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Merge, Validate,
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

    #[merge(strategy = merge::option::recurse)]
    pub description: Option<Replace<String>>,

    #[serde(default = "list_config_default::<Replace<String>>")]
    pub dependencies: ListConfig<Replace<String>>,

    #[serde(default)]
    pub env: ProjectEnvConfiguration,

    #[serde(default)]
    pub cache: CacheConfiguration,

    #[serde(default)]
    pub meta: MetaConfiguration,

    #[serde(default)]
    pub tasks: DictConfig<TaskConfiguration>,
}

impl omni_core::ExtensionGraphNode for ProjectConfiguration {
    type Id = OmniPath;

    fn id(&self) -> &Self::Id {
        &self.file
    }

    fn extendee_ids(&self) -> &[Self::Id] {
        &self.extends
    }
}

impl ProjectConfiguration {
    pub async fn load<'a>(
        path: impl Into<&'a Path>,
        sys: &(impl FsReadAsync + Send + Sync),
    ) -> Result<Self, LoadConfigError> {
        utils::fs::load_config_async(path, sys).await
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
    #[serde(default)]
    pub vars: DictConfig<Replace<String>>,
}
