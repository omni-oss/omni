use config_utils::ListConfig;
use garde::Validate;
use merge::Merge;
use omni_core::TaskDependency;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::Task;

use super::TaskDependencyConfiguration;

fn default_true() -> bool {
    true
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate,
)]
#[serde(untagged)]
#[garde(allow_unvalidated)]
pub enum TaskConfiguration {
    ShortForm(String),
    LongForm {
        command: String,
        #[serde(default)]
        dependencies: ListConfig<TaskDependencyConfiguration>,
        #[serde(default)]
        description: Option<String>,
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
                None,
            ),
            TaskConfiguration::LongForm {
                command,
                dependencies,
                description,
            } => Task::new(
                command,
                dependencies.into_iter().map(Into::into).collect(),
                description,
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
                    description: a_desc,
                },
                Lf {
                    dependencies: b_dep,
                    command: b_cmd,
                    description: b_desc,
                },
            ) => {
                a_dep.merge(b_dep);
                *a_cmd = b_cmd;
                merge::option::overwrite_none(a_desc, b_desc);
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
            description: Some(String::from("a description")),
        };

        let b_tdc = TaskDependencyConfiguration::ExplicitProject {
            project: "project1".to_string(),
            task: "task2".to_string(),
        };

        let b = TaskConfiguration::LongForm {
            command: "b".to_string(),
            dependencies: ListConfig::append(vec![b_tdc.clone()]),
            description: None,
        };

        a.merge(b);

        assert_eq!(
            a,
            TaskConfiguration::LongForm {
                command: "b".to_string(),
                dependencies: ListConfig::value(vec![a_tdc, b_tdc]),
                description: Some(String::from("a description")),
            }
        );
    }
}
