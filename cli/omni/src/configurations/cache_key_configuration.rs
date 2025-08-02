use config_utils::{ListConfig, Replace};
use merge::Merge;
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
    #[serde(default = "super::utils::list_config_default::<Replace<String>>")]
    pub env: ListConfig<Replace<String>>,
    #[serde(default = "super::utils::list_config_default::<Replace<String>>")]
    pub files: ListConfig<Replace<String>>,
}
