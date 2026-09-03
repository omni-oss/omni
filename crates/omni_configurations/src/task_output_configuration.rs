use merge::Merge;
use omni_glob_config::MergeGlobConfig;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    JsonSchema,
    Merge,
)]
#[serde(deny_unknown_fields)]
pub struct TaskOutputConfiguration {
    /// One of three forms: a single pattern, a list of patterns, or an object
    /// with `include` and `exclude` lists. The single and list forms are
    /// include-only. Exclusion is expressed only through `exclude`, which
    /// always wins regardless of order. A leading `!` is a literal character.
    #[serde(default = "super::utils::merge_glob_config_default::<OmniPath>")]
    pub files: MergeGlobConfig<OmniPath>,

    #[serde(default = "super::utils::default_true")]
    #[merge(strategy = config_utils::replace)]
    pub logs: bool,
}

impl Default for TaskOutputConfiguration {
    fn default() -> Self {
        Self {
            files: super::utils::merge_glob_config_default(),
            logs: true,
        }
    }
}
