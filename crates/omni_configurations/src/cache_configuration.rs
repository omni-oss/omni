use config_utils::{ListConfig, Replace};
use merge::Merge;
use omni_glob_config::MergeGlobConfig;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    TaskOutputConfiguration,
    utils::{list_config_default, merge_glob_config_default},
};

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
#[serde(deny_unknown_fields)]
pub struct CacheConfiguration {
    #[serde(default)]
    pub key: CacheKeyConfiguration,
    #[serde(
        default = "default_enabled",
        skip_serializing_if = "Option::is_none"
    )]
    #[merge(strategy = merge::option::recurse)]
    pub enabled: Option<Replace<bool>>,
    /// `false` keeps this task, and every task it depends on, off the remote
    /// cache for the run: it neither reads nor writes remotely. Inherited and
    /// overridable like `enabled`. Release, deploy and signing tasks set it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = merge::option::recurse)]
    pub remote: Option<Replace<bool>>,
    #[serde(default)]
    pub output: TaskOutputConfiguration,
}

#[inline(always)]
fn default_enabled() -> Option<Replace<bool>> {
    None
}

#[inline(always)]
fn default_defaults() -> Option<bool> {
    None
}

impl Default for CacheConfiguration {
    fn default() -> Self {
        Self {
            key: Default::default(),
            enabled: default_enabled(),
            remote: None,
            output: Default::default(),
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
#[serde(deny_unknown_fields)]
pub struct CacheKeyConfiguration {
    #[serde(
        default = "default_defaults",
        skip_serializing_if = "Option::is_none"
    )]
    #[merge(strategy = config_utils::replace_if_some)]
    pub defaults: Option<bool>,

    #[serde(default = "super::utils::list_config_default::<Replace<String>>")]
    pub env: ListConfig<Replace<String>>,

    /// One of three forms: a single pattern, a list of patterns, or an object
    /// with `include` and `exclude` lists. The single and list forms are
    /// include-only. Exclusion is expressed only through `exclude`, which
    /// always wins regardless of order. A leading `!` is a literal character.
    #[serde(default = "super::utils::merge_glob_config_default::<OmniPath>")]
    pub files: MergeGlobConfig<OmniPath>,
}

impl Default for CacheKeyConfiguration {
    #[inline(always)]
    fn default() -> Self {
        Self {
            defaults: default_defaults(),
            env: list_config_default(),
            files: merge_glob_config_default(),
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

    #[test]
    fn test_cache_configuration_merging() {
        let mut a = CacheConfiguration {
            key: CacheKeyConfiguration {
                defaults: Some(false),
                ..Default::default()
            },
            enabled: Some(Replace::new(false)),
            remote: Some(Replace::new(false)),
            output: Default::default(),
        };

        let b = CacheConfiguration {
            key: CacheKeyConfiguration {
                defaults: Some(true),
                ..Default::default()
            },
            enabled: None,
            remote: None,
            output: Default::default(),
        };

        a.merge(b);

        assert_eq!(a.key.defaults, Some(true));
        assert_eq!(a.enabled, Some(Replace::new(false)));
        // An absent `remote` never drops an inherited `false`.
        assert_eq!(a.remote, Some(Replace::new(false)));
    }

    #[test]
    fn remote_false_can_be_overridden_and_is_read_from_config() {
        let mut inherited: CacheConfiguration =
            serde_json::from_str(r#"{"remote": false}"#).unwrap();
        let task_default: CacheConfiguration =
            serde_json::from_str(r#"{}"#).unwrap();
        inherited.merge(task_default);
        assert_eq!(inherited.remote, Some(Replace::new(false)));
        let override_true: CacheConfiguration =
            serde_json::from_str(r#"{"remote": true}"#).unwrap();
        inherited.merge(override_true);
        assert_eq!(inherited.remote, Some(Replace::new(true)));
        assert!(
            serde_json::from_str::<CacheConfiguration>(r#"{"remot": false}"#)
                .is_err()
        );
    }

    #[test]
    fn test_cache_key_deserializes_valid() {
        let result = serde_json::from_str::<CacheKeyConfiguration>(
            r#"{"defaults": true}"#,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_key_rejects_unknown_field() {
        let result = serde_json::from_str::<CacheKeyConfiguration>(
            r#"{"defaults": true, "bogus": 1}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_cache_configuration_rejects_unknown_field() {
        let result = serde_json::from_str::<CacheConfiguration>(
            r#"{"enabled": true, "bogus": 1}"#,
        );
        assert!(result.is_err());
    }
}
