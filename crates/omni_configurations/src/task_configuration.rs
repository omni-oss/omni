use std::time::Duration;

use config_utils::{DictConfig, IntoInner, ListConfig, Replace};
use garde::Validate;
use merge::Merge;
use omni_config_types::TeraExprBoolean;
use omni_core::{Task, TaskDependency};
use omni_serde_validators::tera_expr::validate_tera_expr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CacheConfiguration, MetaConfiguration, TaskOutputConfiguration};

use super::TaskDependencyConfiguration;

#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate,
)]
#[garde(allow_unvalidated)]
pub struct TaskConfigurationLongForm {
    #[serde(default, deserialize_with = "validate_tera_expr")]
    pub command: String,
    #[serde(
        default = "super::utils::list_config_default::<TaskDependencyConfiguration>"
    )]
    pub dependencies: ListConfig<TaskDependencyConfiguration>,

    #[serde(
        default = "super::utils::list_config_default::<TaskDependencyConfiguration>"
    )]
    pub with: ListConfig<TaskDependencyConfiguration>,

    #[serde(default)]
    pub description: Option<Replace<String>>,

    #[serde(default = "default_if", alias = "if")]
    pub enabled: Option<TeraExprBoolean>,

    #[serde(default = "default_interactive")]
    pub interactive: Option<Replace<bool>>,

    #[serde(default = "default_persistent")]
    pub persistent: Option<Replace<bool>>,

    #[serde(default)]
    pub env: TaskEnvConfiguration,

    #[serde(default)]
    pub cache: CacheConfiguration,

    #[serde(default)]
    pub output: TaskOutputConfiguration,

    #[serde(default)]
    pub meta: MetaConfiguration,

    #[serde(default)]
    pub max_retries: Option<Replace<u8>>,

    #[serde(default, with = "retry_interval")]
    #[schemars(with = "Option<Replace<String>>")]
    pub retry_interval: Option<Replace<Duration>>,
}

mod retry_interval {
    use std::time::Duration;

    use config_utils::{AsInner, Replace};
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<Replace<Duration>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let string = Option::<Replace<String>>::deserialize(deserializer)?;

        if let Some(string) = string {
            let duration = humantime::parse_duration(string.as_inner())
                .map_err(serde::de::Error::custom)?;
            Ok(Some(Replace::new(duration)))
        } else {
            Ok(None)
        }
    }

    pub fn serialize<S>(
        value: &Option<Replace<Duration>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(duration) = value {
            serializer.serialize_str(
                &humantime::format_duration(*duration.as_inner()).to_string(),
            )
        } else {
            serializer.serialize_none()
        }
    }
}

#[inline(always)]
fn default_if() -> Option<TeraExprBoolean> {
    Some(TeraExprBoolean::Boolean(true))
}

#[inline(always)]
fn default_persistent() -> Option<Replace<bool>> {
    None
}

#[inline(always)]
fn default_interactive() -> Option<Replace<bool>> {
    None
}

impl Default for TaskConfigurationLongForm {
    fn default() -> Self {
        Self {
            command: String::new(),
            dependencies: ListConfig::append(vec![]),
            description: None,
            env: TaskEnvConfiguration::default(),
            cache: CacheConfiguration::default(),
            output: TaskOutputConfiguration::default(),
            meta: MetaConfiguration::default(),
            enabled: default_if(),
            interactive: default_interactive(),
            persistent: default_persistent(),
            with: ListConfig::append(vec![]),
            max_retries: None,
            retry_interval: None,
        }
    }
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
                true.into(),
                false,
                false,
                vec![],
                None,
                None,
            ),
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                command,
                dependencies,
                description,
                enabled,
                interactive,
                persistent,
                with,
                max_retries: retries,
                retry_interval,
                ..
            }) => Task::new(
                command.clone(),
                dependencies.iter().cloned().map(Into::into).collect(),
                description.clone().map(|e| e.into_inner()),
                enabled.clone().unwrap_or(true.into()),
                interactive.map(|e| e.into_inner()).unwrap_or(false),
                persistent.map(|e| e.into_inner()).unwrap_or(false),
                with.iter().cloned().map(Into::into).collect(),
                retries.map(|e| e.into_inner()),
                retry_interval.map(|e| e.into_inner()),
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
                    cache: a_cache,
                    output: a_output,
                    meta: a_meta,
                    enabled: a_enabled,
                    interactive: a_interactive,
                    persistent: a_persistent,
                    with: a_with,
                    max_retries: a_retries,
                    retry_interval: a_retry_interval,
                }),
                Lf(box TaskConfigurationLongForm {
                    dependencies: b_dep,
                    command: b_cmd,
                    description: b_desc,
                    env: b_env,
                    cache: b_cache,
                    output: b_output,
                    meta: b_meta,
                    enabled: b_enabled,
                    interactive: b_interactive,
                    persistent: b_persistent,
                    with: b_with,
                    max_retries: b_retries,
                    retry_interval: b_retry_interval,
                }),
            ) => {
                a_dep.merge(b_dep);
                if !b_cmd.trim().is_empty() {
                    *a_cmd = b_cmd;
                }
                merge::option::recurse(a_desc, b_desc);
                a_env.merge(b_env);
                a_cache.merge(b_cache);
                a_output.merge(b_output);
                a_meta.merge(b_meta);
                merge::option::recurse(a_enabled, b_enabled);
                merge::option::recurse(a_interactive, b_interactive);
                merge::option::recurse(a_persistent, b_persistent);
                a_with.merge(b_with);
                merge::option::recurse(a_retries, b_retries);
                merge::option::recurse(a_retry_interval, b_retry_interval);
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
            description: Some(Replace::new(String::from("a description"))),
            env: Default::default(),
            cache: Default::default(),
            output: TaskOutputConfiguration::default(),
            meta: Default::default(),
            interactive: Some(Replace::new(false)),
            persistent: Some(Replace::new(true)),
            enabled: Some(true.into()),
            with: ListConfig::append(vec![]),
            max_retries: Some(Replace::new(1)),
            retry_interval: Some(Replace::new(Duration::from_secs(1))),
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
            interactive: Some(Replace::new(true)),
            persistent: Some(Replace::new(false)),
            enabled: None,
            with: ListConfig::append(vec![]),
            max_retries: Some(Replace::new(3)),
            retry_interval: Some(Replace::new(Duration::from_secs(2))),
        });

        a.merge(b);

        assert_eq!(
            a,
            TaskConfiguration::long_form(TaskConfigurationLongForm {
                command: "b".to_string(),
                dependencies: ListConfig::append(vec![a_tdc, b_tdc]),
                description: Some(Replace::new(String::from("a description"))),
                env: Default::default(),
                cache: Default::default(),
                output: Default::default(),
                meta: Default::default(),
                interactive: Some(Replace::new(true)),
                persistent: Some(Replace::new(false)),
                enabled: Some(true.into()),
                with: ListConfig::append(vec![]),
                max_retries: Some(Replace::new(3)),
                retry_interval: Some(Replace::new(Duration::from_secs(2))),
            })
        );
    }
}
