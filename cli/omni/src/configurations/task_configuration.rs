use omni_core::TaskDependency;
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
    },
}

impl TaskConfiguration {
    pub fn into_task(self, name: &str) -> Task {
        match self {
            TaskConfiguration::ShortForm(command) => Task::new(
                command,
                vec![TaskDependency::Upstream {
                    task: name.to_string(),
                }],
            ),
            TaskConfiguration::LongForm {
                command,
                dependencies,
            } => Task::new(
                command,
                dependencies.into_iter().map(Into::into).collect(),
            ),
        }
    }
}
