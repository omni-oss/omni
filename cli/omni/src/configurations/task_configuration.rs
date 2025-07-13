use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::Task;

use super::TaskDependencyConfiguration;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TaskConfiguration {
    ShortForm(String),
    LongForm {
        command: String,
        #[serde(default)]
        dependencies: Vec<TaskDependencyConfiguration>,
        #[serde(default = "default_true")]
        merge_project_dependencies: bool,
    },
}

impl TaskConfiguration {
    pub fn should_merge_project_dependencies(&self) -> bool {
        match self {
            TaskConfiguration::ShortForm(_) => true,
            TaskConfiguration::LongForm {
                merge_project_dependencies,
                ..
            } => *merge_project_dependencies,
        }
    }
}

impl From<TaskConfiguration> for Task {
    fn from(val: TaskConfiguration) -> Self {
        match val {
            TaskConfiguration::ShortForm(r) => Task::new(r, vec![]),
            TaskConfiguration::LongForm {
                command,
                dependencies,
                ..
            } => Task::new(
                command,
                dependencies.into_iter().map(|d| d.into()).collect(),
            ),
        }
    }
}
