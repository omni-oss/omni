use config_utils::{ListConfig, Replace};
use merge::Merge;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::configurations::utils::list_config_default;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    Merge,
    PartialOrd,
    Ord,
)]
pub struct CacheConfiguration {
    #[serde(default)]
    pub key: CacheKeyConfiguration,
    #[serde(default = "super::utils::default_true")]
    #[merge(strategy = config_utils::replace)]
    pub enabled: bool,
}

impl Default for CacheConfiguration {
    fn default() -> Self {
        Self {
            key: Default::default(),
            enabled: true,
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    Merge,
    PartialOrd,
    Ord,
)]
pub struct CacheKeyConfiguration {
    #[serde(default = "super::utils::default_true")]
    #[merge(strategy = config_utils::replace)]
    pub defaults: bool,

    #[serde(default = "super::utils::list_config_default::<Replace<String>>")]
    pub env: ListConfig<Replace<String>>,

    #[serde(default = "super::utils::list_config_default::<OmniPath>")]
    pub files: ListConfig<OmniPath>,
}

impl Default for CacheKeyConfiguration {
    fn default() -> Self {
        Self {
            defaults: true,
            env: list_config_default(),
            files: list_config_default(),
        }
    }
}
