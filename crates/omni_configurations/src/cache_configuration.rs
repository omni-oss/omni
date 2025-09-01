use config_utils::{ListConfig, Replace};
use merge::Merge;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::utils::list_config_default;

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
    #[serde(default)]
    #[merge(strategy = config_utils::replace_if_some)]
    pub defaults: Option<bool>,

    #[serde(default = "super::utils::list_config_default::<Replace<String>>")]
    pub env: ListConfig<Replace<String>>,

    #[serde(default = "super::utils::list_config_default::<OmniPath>")]
    pub files: ListConfig<OmniPath>,
}

impl Default for CacheKeyConfiguration {
    fn default() -> Self {
        Self {
            defaults: None,
            env: list_config_default(),
            files: list_config_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_cache_key_merge_defaults() {
        // It should only replace if the right side is Some
        let mut default = CacheKeyConfiguration {
            defaults: Some(true),
            ..Default::default()
        };
        let custom = CacheKeyConfiguration {
            defaults: None,
            ..Default::default()
        };

        let mut default2 = CacheKeyConfiguration {
            defaults: Some(true),
            ..Default::default()
        };

        let custom2 = CacheKeyConfiguration {
            defaults: Some(false),
            ..Default::default()
        };

        default.merge(custom);
        default2.merge(custom2);
        assert_eq!(default.defaults, Some(true));
        assert_eq!(default2.defaults, Some(false));
    }
}
