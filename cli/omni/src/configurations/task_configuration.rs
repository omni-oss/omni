use config_utils::ListConfig;
use merge::Merge;
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
        dependencies: ListConfig<TaskDependencyConfiguration>,
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

impl Merge for TaskConfiguration {
    fn merge(&mut self, other: Self) {
        use TaskConfiguration::{LongForm as Lf, ShortForm as Sf};
        match (self, other) {
            (
                Lf {
                    dependencies: a_dep,
                    command: a_cmd,
                },
                Lf {
                    dependencies: b_dep,
                    command: b_cmd,
                },
            ) => {
                a_dep.merge(b_dep);
                *a_cmd = b_cmd;
            }
            (this @ Lf { .. }, other @ Sf(..))
            | (this @ Sf { .. }, other @ Lf { .. })
            | (this @ Sf { .. }, other @ Sf { .. }) => *this = other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_short_form() {
        let mut a = TaskConfiguration::ShortForm("a".to_string());
        let b = TaskConfiguration::ShortForm("b".to_string());

        a.merge(b);

        assert_eq!(a, TaskConfiguration::ShortForm("b".to_string()));
    }

    #[test]
    fn test_merge_long_form() {
        let a_tdc = TaskDependencyConfiguration::Own {
            task: "task1".to_string(),
        };

        let mut a = TaskConfiguration::LongForm {
            command: "a".to_string(),
            dependencies: ListConfig::value(vec![a_tdc.clone()]),
        };

        let b_tdc = TaskDependencyConfiguration::ExplicitProject {
            project: "project1".to_string(),
            task: "task2".to_string(),
        };

        let b = TaskConfiguration::LongForm {
            command: "b".to_string(),
            dependencies: ListConfig::append(vec![b_tdc.clone()]),
        };

        a.merge(b);

        assert_eq!(
            a,
            TaskConfiguration::LongForm {
                command: "b".to_string(),
                dependencies: ListConfig::value(vec![a_tdc, b_tdc]),
            }
        );
    }
}
