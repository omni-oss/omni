use config_utils::{DictConfig, ListConfig, Replace};
use garde::Validate;
use merge::Merge;
use omni_core::TaskDependency;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    configurations::{
        CacheConfiguration, MetaConfiguration, TaskOutputConfiguration,
    },
    core::Task,
};

use super::TaskDependencyConfiguration;

#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate,
)]
#[garde(allow_unvalidated)]
pub struct TaskConfigurationLongForm {
    #[serde(default)]
    pub command: String,
    #[serde(
        default = "super::utils::list_config_default::<TaskDependencyConfiguration>"
    )]
    pub dependencies: ListConfig<TaskDependencyConfiguration>,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub env: TaskEnvConfiguration,

    #[serde(default)]
    pub cache: CacheConfiguration,

    #[serde(default)]
    pub output: TaskOutputConfiguration,

    #[serde(default)]
    pub meta: MetaConfiguration,
}

#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate,
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
pub struct TaskEnvConfiguration {
    #[merge(strategy = merge::option::recurse)]
    pub vars: Option<DictConfig<Replace<String>>>,
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

    pub fn cache(&self) -> Option<&CacheConfiguration> {
        match self {
            TaskConfiguration::ShortForm(_) => None,
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                cache,
                ..
            }) => Some(cache),
        }
    }

    pub fn output(&self) -> Option<&TaskOutputConfiguration> {
        match self {
            TaskConfiguration::ShortForm(_) => None,
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                output,
                ..
            }) => Some(output),
        }
    }

    pub fn env(&self) -> Option<&TaskEnvConfiguration> {
        match self {
            TaskConfiguration::ShortForm(_) => None,
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                env,
                ..
            }) => Some(env),
        }
    }

    pub fn meta(&self) -> Option<&MetaConfiguration> {
        match self {
            TaskConfiguration::ShortForm(_) => None,
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                meta,
                ..
            }) => Some(meta),
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
                    cache: a_cache_key,
                    output: a_output,
                    meta: a_meta,
                }),
                Lf(box TaskConfigurationLongForm {
                    dependencies: b_dep,
                    command: b_cmd,
                    description: b_desc,
                    env: b_env,
                    cache: b_cache_key,
                    output: b_output,
                    meta: b_meta,
                }),
            ) => {
                a_dep.merge(b_dep);
                if !b_cmd.trim().is_empty() {
                    *a_cmd = b_cmd;
                }
                merge::option::overwrite_none(a_desc, b_desc);
                a_env.merge(b_env);
                a_cache_key.merge(b_cache_key);
                a_output.merge(b_output);
                a_meta.merge(b_meta);
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
            env: Default::default(),
            cache: Default::default(),
            output: TaskOutputConfiguration::default(),
            meta: Default::default(),
        });

        let b_tdc = TaskDependencyConfiguration::ExplicitProject {
            project: "project1".to_string(),
            task: "task2".to_string(),
        };

        let b = TaskConfiguration::long_form(TaskConfigurationLongForm {
            command: "b".to_string(),
            dependencies: ListConfig::append(vec![b_tdc.clone()]),
            description: None,
            env: Default::default(),
            cache: Default::default(),
            output: TaskOutputConfiguration::default(),
            meta: Default::default(),
        });

        a.merge(b);

        assert_eq!(
            a,
            TaskConfiguration::long_form(TaskConfigurationLongForm {
                command: "b".to_string(),
                dependencies: ListConfig::append(vec![a_tdc, b_tdc]),
                description: Some(String::from("a description")),
                env: Default::default(),
                cache: Default::default(),
                output: Default::default(),
                meta: Default::default(),
            })
        );
    }
}
