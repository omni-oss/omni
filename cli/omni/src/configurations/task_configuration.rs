use config_utils::{DictConfig, ListConfig, Replace};
use garde::Validate;
use merge::Merge;
use omni_core::TaskDependency;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    configurations::{CacheKeyConfiguration, TaskOutputConfiguration},
    core::Task,
};

use super::TaskDependencyConfiguration;

fn default_true() -> bool {
    true
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate,
)]
#[garde(allow_unvalidated)]
pub struct TaskConfigurationLongForm {
    command: String,
    #[serde(
        default = "super::utils::list_config_default::<TaskDependencyConfiguration>"
    )]
    dependencies: ListConfig<TaskDependencyConfiguration>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    env: Option<TaskConfigurationEnvConfiguration>,
    #[serde(default)]
    cache_key: Option<CacheKeyConfiguration>,
    #[serde(default)]
    output: TaskOutputConfiguration,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Validate,
)]
#[serde(untagged)]
#[garde(allow_unvalidated)]
pub enum TaskConfiguration {
    ShortForm(String),
    LongForm(Box<TaskConfigurationLongForm>),
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
    Default,
)]
pub struct TaskConfigurationEnvConfiguration {
    pub overrides: DictConfig<Replace<String>>,
}

impl TaskConfiguration {
    pub fn short_form(command: String) -> Self {
        Self::ShortForm(command)
    }

    pub fn long_form(long_form: TaskConfigurationLongForm) -> Self {
        Self::LongForm(Box::new(long_form))
    }
}

impl TaskConfiguration {
    pub fn get_task(&self, name: &str) -> Task {
        match self {
            TaskConfiguration::ShortForm(command) => Task::new(
                command.clone(),
                vec![TaskDependency::Upstream {
                    task: name.to_string(),
                }],
                None,
            ),
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                command,
                dependencies,
                description,
                ..
            }) => Task::new(
                command.clone(),
                dependencies.iter().cloned().map(Into::into).collect(),
                description.clone(),
            ),
        }
    }
}

impl Merge for TaskConfiguration {
    fn merge(&mut self, other: Self) {
        use TaskConfiguration::{LongForm as Lf, ShortForm as Sf};
        match (self, other) {
            (
                Lf(box TaskConfigurationLongForm {
                    dependencies: a_dep,
                    command: a_cmd,
                    description: a_desc,
                    env: a_env,
                    cache_key: a_cache_key,
                    output: a_output,
                }),
                Lf(box TaskConfigurationLongForm {
                    dependencies: b_dep,
                    command: b_cmd,
                    description: b_desc,
                    env: b_env,
                    cache_key: b_cache_key,
                    output: b_output,
                }),
            ) => {
                a_dep.merge(b_dep);
                *a_cmd = b_cmd;
                merge::option::overwrite_none(a_desc, b_desc);
                merge::option::recurse(a_env, b_env);
                merge::option::recurse(a_cache_key, b_cache_key);
                a_output.merge(b_output);
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

        let mut a = TaskConfiguration::long_form(TaskConfigurationLongForm {
            command: "a".to_string(),
            dependencies: ListConfig::value(vec![a_tdc.clone()]),
            description: Some(String::from("a description")),
            env: None,
            cache_key: None,
            output: TaskOutputConfiguration::default(),
        });

        let b_tdc = TaskDependencyConfiguration::ExplicitProject {
            project: "project1".to_string(),
            task: "task2".to_string(),
        };

        let b = TaskConfiguration::long_form(TaskConfigurationLongForm {
            command: "b".to_string(),
            dependencies: ListConfig::append(vec![b_tdc.clone()]),
            description: None,
            env: None,
            cache_key: None,
            output: TaskOutputConfiguration::default(),
        });

        a.merge(b);

        assert_eq!(
            a,
            TaskConfiguration::long_form(TaskConfigurationLongForm {
                command: "b".to_string(),
                dependencies: ListConfig::append(vec![a_tdc, b_tdc]),
                description: Some(String::from("a description")),
                env: None,
                cache_key: None,
                output: TaskOutputConfiguration::default(),
            })
        );
    }
}
