use config_utils::{ListConfig, Replace};
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Merge,
)]
pub struct TaskOutputConfiguration {
    #[serde(default = "super::utils::list_config_default::<Replace<String>>")]
    pub files: ListConfig<Replace<String>>,

    #[serde(default = "super::utils::default_true")]
    #[merge(strategy = config_utils::replace)]
    pub stdout: bool,

    #[serde(default = "super::utils::default_true")]
    #[merge(strategy = config_utils::replace)]
    pub stderr: bool,
}

impl Default for TaskOutputConfiguration {
    fn default() -> Self {
        Self {
            files: ListConfig::append(vec![]),
            stdout: true,
            stderr: true,
        }
    }
}
