use config_utils::{DictConfig, Replace};
use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    Default,
)]
#[garde(allow_unvalidated)]
pub struct MetaConfiguration {
    #[serde(default)]
    #[merge(strategy = merge::option::recurse)]
    pub category: Option<Replace<String>>,

    #[serde(default)]
    pub tags: DictConfig<Replace<String>>,
}
