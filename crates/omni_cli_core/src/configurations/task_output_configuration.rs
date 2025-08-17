use config_utils::ListConfig;
use merge::Merge;
use omni_types::OmniPath;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Merge,
)]
pub struct TaskOutputConfiguration {
    #[serde(default = "super::utils::list_config_default::<OmniPath>")]
    pub files: ListConfig<OmniPath>,

    #[serde(default = "super::utils::default_true")]
    #[merge(strategy = config_utils::replace)]
    pub logs: bool,
}

impl Default for TaskOutputConfiguration {
    fn default() -> Self {
        Self {
            files: ListConfig::append(vec![]),
            logs: true,
        }
    }
}
