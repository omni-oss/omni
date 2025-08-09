use config_utils::{ListConfig, Replace};
use merge::Merge;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    Merge,
    Default,
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
