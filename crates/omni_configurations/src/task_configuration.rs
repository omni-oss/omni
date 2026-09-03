use std::time::Duration;

use config_utils::{DictConfig, DynValue, IntoInner, ListConfig, Replace};
use garde::Validate;
use merge::Merge;
use omni_command_config::CommandConfig;
use omni_config_types::{SingleOrMany, TeraExprBoolean};
use omni_core::Task;
use omni_task_output_logs::OutputLogsConfiguration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CacheConfiguration, MetaConfiguration};

use super::TaskDependencyConfiguration;

#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Validate,
)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub struct TaskConfigurationLongForm {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub base: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<SingleOrMany<String>>,

    #[serde(
        default,
        alias = "command",
        skip_serializing_if = "Option::is_none"
    )]
    pub exec: Option<Replace<CommandConfig>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_exec: Option<Replace<CommandConfig>>,

    #[serde(default)]
    pub args: DictConfig<DynValue>,

    #[serde(
        default = "super::utils::list_config_default::<TaskDependencyConfiguration>"
    )]
    pub dependencies: ListConfig<TaskDependencyConfiguration>,

    #[serde(
        default = "super::utils::list_config_default::<TaskDependencyConfiguration>"
    )]
    pub with: ListConfig<TaskDependencyConfiguration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<Replace<String>>,

    #[serde(
        default = "default_if",
        alias = "if",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled: Option<TeraExprBoolean>,

    #[serde(
        default = "default_interactive",
        skip_serializing_if = "Option::is_none"
    )]
    pub interactive: Option<Replace<bool>>,

    #[serde(
        default = "default_persistent",
        skip_serializing_if = "Option::is_none"
    )]
    pub persistent: Option<Replace<bool>>,

    #[serde(default)]
    pub env: TaskEnvConfiguration,

    #[serde(default)]
    pub cache: CacheConfiguration,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_logs: Option<OutputLogsConfiguration>,

    #[serde(default)]
    pub meta: MetaConfiguration,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<Replace<u8>>,

    #[serde(
        default,
        with = "retry_interval",
        skip_serializing_if = "Option::is_none"
    )]
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
    None
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
            base: false,
            extends: None,
            exec: None,
            retry_exec: None,
            dependencies: ListConfig::append(vec![]),
            description: None,
            args: DictConfig::default(),
            env: TaskEnvConfiguration::default(),
            cache: CacheConfiguration::default(),
            output_logs: None,
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

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema, Validate)]
#[serde(untagged)]
#[garde(allow_unvalidated)]
pub enum TaskConfiguration {
    ShortForm(CommandConfig),
    LongForm(Box<TaskConfigurationLongForm>),
}

impl<'a> Deserialize<'a> for TaskConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        use omni_serde_validators::tera_expr::validate_str;
        serde_untagged::UntaggedEnumVisitor::new()
            .string(|s| {
                validate_str(&s).map_err(serde::de::Error::custom)?;
                Ok(TaskConfiguration::ShortForm(CommandConfig::Shell(
                    s.to_string(),
                )))
            })
            .seq(|seq| {
                let items: Vec<String> = seq.deserialize()?;
                for item in &items {
                    validate_str(item).map_err(serde::de::Error::custom)?;
                }
                Ok(TaskConfiguration::ShortForm(CommandConfig::Argv(items)))
            })
            .map(|long_form| {
                long_form.deserialize().map(TaskConfiguration::LongForm)
            })
            .deserialize(deserializer)
    }
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
#[serde(deny_unknown_fields)]
pub struct TaskEnvConfiguration {
    #[merge(strategy = merge::option::recurse)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars: Option<DictConfig<Replace<String>>>,
}

impl TaskConfiguration {
    pub fn short_form(command: impl Into<CommandConfig>) -> Self {
        Self::ShortForm(command.into())
    }

    pub fn long_form(long_form: TaskConfigurationLongForm) -> Self {
        Self::LongForm(Box::new(long_form))
    }
}

impl TaskConfigurationLongForm {
    /// Lower this long-form configuration into an executable [`Task`].
    ///
    /// This is the single lowering path; [`TaskConfiguration::get_task`] reaches
    /// it via [`TaskConfiguration::into_long_form`].
    pub fn into_task(self) -> Task {
        let TaskConfigurationLongForm {
            exec: command,
            retry_exec: retry_command,
            dependencies,
            description,
            enabled,
            interactive,
            persistent,
            with,
            max_retries: retries,
            retry_interval,
            ..
        } = self;

        Task::new(
            command.map(|x| x.into_inner()),
            retry_command.map(|x| x.into_inner()),
            dependencies.iter().cloned().map(Into::into).collect(),
            description.map(|e| e.into_inner()),
            enabled.unwrap_or(true.into()),
            interactive.map(|e| e.into_inner()).unwrap_or(false),
            persistent.map(|e| e.into_inner()).unwrap_or(false),
            with.iter().cloned().map(Into::into).collect(),
            retries.map(|e| e.into_inner()),
            retry_interval.map(|e| e.into_inner()),
        )
    }
}

impl TaskConfiguration {
    /// Canonical long-form expansion of this task.
    ///
    /// A short-form task `X: cmd` expands to
    /// `{ exec: cmd, dependencies: ["^X"] }` — preserving the implicit upstream
    /// self-dependency that short-form tasks carry (see [`Self::get_task`]).
    /// Long-form tasks are returned unchanged. This is the single source of
    /// truth for short-form semantics: [`Self::get_task`] routes through it, so
    /// a short-form task and its long-form expansion are equal by construction.
    pub fn into_long_form(self, name: &str) -> TaskConfigurationLongForm {
        match self {
            TaskConfiguration::LongForm(long_form) => *long_form,
            TaskConfiguration::ShortForm(command) => {
                TaskConfigurationLongForm {
                    exec: Some(Replace::new(command)),
                    dependencies: ListConfig::append(vec![
                        TaskDependencyConfiguration::Upstream {
                            task: name.to_string(),
                        },
                    ]),
                    ..Default::default()
                }
            }
        }
    }

    pub fn get_task(&self, name: &str) -> Task {
        self.clone().into_long_form(name).into_task()
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

    pub fn output_logs(&self) -> Option<&OutputLogsConfiguration> {
        match self {
            TaskConfiguration::ShortForm(_) => None,
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                output_logs,
                ..
            }) => output_logs.as_ref(),
        }
    }

    pub fn args(&self) -> Option<&DictConfig<DynValue>> {
        match self {
            TaskConfiguration::ShortForm(_) => None,
            TaskConfiguration::LongForm(box TaskConfigurationLongForm {
                args,
                ..
            }) => Some(args),
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

impl Merge for TaskConfigurationLongForm {
    fn merge(&mut self, other: Self) {
        let TaskConfigurationLongForm {
            base: b_base,
            extends: b_extends,
            dependencies: b_dep,
            exec: b_cmd,
            retry_exec: b_retry_cmd,
            description: b_desc,
            env: b_env,
            cache: b_cache,
            output_logs: b_output_logs,
            meta: b_meta,
            enabled: b_enabled,
            interactive: b_interactive,
            persistent: b_persistent,
            with: b_with,
            max_retries: b_retries,
            retry_interval: b_retry_interval,
            args: b_args,
        } = other;

        // `base` is a per-declaration structural marker ("is this block a
        // template?"), not inheritable data, so it is always replaced and never
        // inherited through `extends`. Inheriting it would make a concrete task
        // that extends a `base: true` template itself become a dropped template.
        self.base = b_base;
        // Unlike `base`, `extends` is inheritable: an overriding layer that omits
        // it (or sets it to null) must NOT clear the base's `extends`, otherwise
        // partially overriding a task (e.g. only to tweak an env var) silently
        // drops its inherited task extension. Only replace when the override
        // actually provides one, matching how `exec`/`enabled`/`description`/etc.
        // behave via `option::recurse`.
        config_utils::replace_if_some(&mut self.extends, b_extends);
        self.dependencies.merge(b_dep);
        merge::option::recurse(&mut self.exec, b_cmd);
        merge::option::recurse(&mut self.retry_exec, b_retry_cmd);
        merge::option::recurse(&mut self.description, b_desc);
        self.env.merge(b_env);
        self.cache.merge(b_cache);
        merge::option::recurse(&mut self.output_logs, b_output_logs);
        self.meta.merge(b_meta);
        self.args.merge(b_args);
        merge::option::recurse(&mut self.enabled, b_enabled);
        merge::option::recurse(&mut self.interactive, b_interactive);
        merge::option::recurse(&mut self.persistent, b_persistent);
        self.with.merge(b_with);
        merge::option::recurse(&mut self.max_retries, b_retries);
        merge::option::recurse(&mut self.retry_interval, b_retry_interval);
    }
}

impl Merge for TaskConfiguration {
    fn merge(&mut self, other: Self) {
        use TaskConfiguration::{LongForm as Lf, ShortForm as Sf};
        match (self, other) {
            (Lf(a), Lf(b)) => a.merge(*b),
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
        let mut a =
            TaskConfiguration::ShortForm(CommandConfig::Shell("a".to_string()));
        let b =
            TaskConfiguration::ShortForm(CommandConfig::Shell("b".to_string()));

        a.merge(b);

        assert_eq!(
            a,
            TaskConfiguration::ShortForm(CommandConfig::Shell("b".to_string()))
        );
    }

    #[test]
    fn test_short_form_string_is_shell_sequence_is_argv() {
        let shell: TaskConfiguration =
            serde_json::from_str(r#""echo hi""#).unwrap();
        assert_eq!(
            shell,
            TaskConfiguration::ShortForm(CommandConfig::Shell(
                "echo hi".to_string()
            ))
        );

        let argv: TaskConfiguration =
            serde_json::from_str(r#"["echo", "a b"]"#).unwrap();
        assert_eq!(
            argv,
            TaskConfiguration::ShortForm(CommandConfig::Argv(vec![
                "echo".to_string(),
                "a b".to_string(),
            ]))
        );
    }

    /// The canonical long-form expansion of a short-form shell task must be
    /// `{ exec: cmd, dependencies: ["^<name>"] }`, including the implicit
    /// upstream self-dependency that short-form tasks carry.
    #[test]
    fn test_short_form_into_long_form_shell() {
        let short = TaskConfiguration::ShortForm(CommandConfig::Shell(
            "cargo build".to_string(),
        ));

        let expected = TaskConfigurationLongForm {
            exec: Some(Replace::new(CommandConfig::Shell(
                "cargo build".to_string(),
            ))),
            dependencies: ListConfig::append(vec![
                TaskDependencyConfiguration::Upstream {
                    task: "build".to_string(),
                },
            ]),
            ..Default::default()
        };

        assert_eq!(short.into_long_form("build"), expected);
    }

    #[test]
    fn test_short_form_into_long_form_argv() {
        let short = TaskConfiguration::ShortForm(CommandConfig::Argv(vec![
            "cargo".to_string(),
            "build".to_string(),
        ]));

        let expected = TaskConfigurationLongForm {
            exec: Some(Replace::new(CommandConfig::Argv(vec![
                "cargo".to_string(),
                "build".to_string(),
            ]))),
            dependencies: ListConfig::append(vec![
                TaskDependencyConfiguration::Upstream {
                    task: "build".to_string(),
                },
            ]),
            ..Default::default()
        };

        assert_eq!(short.into_long_form("build"), expected);
    }

    /// `into_long_form` must be the identity on long-form tasks.
    #[test]
    fn test_long_form_into_long_form_is_identity() {
        let long_form = TaskConfigurationLongForm {
            exec: Some(Replace::new(CommandConfig::Shell(
                "cargo test".to_string(),
            ))),
            dependencies: ListConfig::value(vec![
                TaskDependencyConfiguration::Own {
                    task: "build".to_string(),
                },
            ]),
            ..Default::default()
        };

        let task = TaskConfiguration::long_form(long_form.clone());

        assert_eq!(task.into_long_form("test"), long_form);
    }

    /// Hard-requirement gate: a short-form task and its long-form expansion must
    /// lower to an identical `Task`. This proves auto-promotion (which reuses
    /// `into_long_form`) is behavior-preserving.
    #[test]
    fn test_short_form_get_task_equals_long_form_expansion() {
        for command in [
            CommandConfig::Shell("cargo build".to_string()),
            CommandConfig::Argv(vec!["cargo".to_string(), "build".to_string()]),
        ] {
            let short = TaskConfiguration::ShortForm(command.clone());

            let expanded =
                TaskConfiguration::long_form(TaskConfigurationLongForm {
                    exec: Some(Replace::new(command)),
                    dependencies: ListConfig::append(vec![
                        TaskDependencyConfiguration::Upstream {
                            task: "build".to_string(),
                        },
                    ]),
                    ..Default::default()
                });

            assert_eq!(
                short.get_task("build"),
                expanded.get_task("build"),
                "short-form and its long-form expansion must lower identically"
            );
        }
    }

    /// The implicit upstream self-dependency must survive lowering.
    #[test]
    fn test_short_form_get_task_has_implicit_upstream_dependency() {
        let short = TaskConfiguration::ShortForm(CommandConfig::Shell(
            "cargo build".to_string(),
        ));

        let task = short.get_task("build");

        assert_eq!(
            task.dependencies,
            vec![omni_core::TaskDependency::Upstream {
                task: "build".to_string(),
            }],
        );
    }

    /// A naive `{ exec: cmd }` long form (no dependencies) must NOT equal the
    /// short-form lowering — guards against a regression that drops the implicit
    /// upstream dependency.
    #[test]
    fn test_short_form_not_equal_to_bare_exec_long_form() {
        let short = TaskConfiguration::ShortForm(CommandConfig::Shell(
            "cargo build".to_string(),
        ));

        let bare = TaskConfiguration::long_form(TaskConfigurationLongForm {
            exec: Some(Replace::new(CommandConfig::Shell(
                "cargo build".to_string(),
            ))),
            ..Default::default()
        });

        assert_ne!(short.get_task("build"), bare.get_task("build"));
    }

    #[test]
    fn test_merge_long_form() {
        let a_tdc = TaskDependencyConfiguration::Own {
            task: "task1".to_string(),
        };

        let mut a = TaskConfiguration::long_form(TaskConfigurationLongForm {
            exec: Some(Replace::new(CommandConfig::Shell("a".to_string()))),
            dependencies: ListConfig::value(vec![a_tdc.clone()]),
            description: Some(Replace::new(String::from("a description"))),
            env: Default::default(),
            cache: Default::default(),
            meta: Default::default(),
            interactive: Some(Replace::new(false)),
            persistent: Some(Replace::new(true)),
            enabled: Some(true.into()),
            with: ListConfig::append(vec![]),
            max_retries: Some(Replace::new(1)),
            retry_interval: Some(Replace::new(Duration::from_secs(1))),
            ..Default::default()
        });

        let b_tdc = TaskDependencyConfiguration::ExplicitProject {
            project: "project1".to_string(),
            task: "task2".to_string(),
        };

        let b = TaskConfiguration::long_form(TaskConfigurationLongForm {
            exec: Some(Replace::new(CommandConfig::Shell("b".to_string()))),
            dependencies: ListConfig::append(vec![b_tdc.clone()]),
            description: None,
            env: Default::default(),
            cache: Default::default(),
            meta: Default::default(),
            interactive: Some(Replace::new(true)),
            persistent: Some(Replace::new(false)),
            enabled: None,
            with: ListConfig::append(vec![]),
            max_retries: Some(Replace::new(3)),
            retry_interval: Some(Replace::new(Duration::from_secs(2))),
            ..Default::default()
        });

        a.merge(b);

        // Merging two configs normalizes each glob field into an explicit
        // include/exclude pair, so the merged cache is default-merged-default
        // rather than the bare default.
        let mut merged_cache = CacheConfiguration::default();
        merged_cache.merge(CacheConfiguration::default());

        assert_eq!(
            a,
            TaskConfiguration::long_form(TaskConfigurationLongForm {
                exec: Some(Replace::new(CommandConfig::Shell("b".to_string()))),
                dependencies: ListConfig::append(vec![a_tdc, b_tdc]),
                description: Some(Replace::new(String::from("a description"))),
                env: Default::default(),
                cache: merged_cache,
                meta: Default::default(),
                interactive: Some(Replace::new(true)),
                persistent: Some(Replace::new(false)),
                enabled: Some(true.into()),
                with: ListConfig::append(vec![]),
                max_retries: Some(Replace::new(3)),
                retry_interval: Some(Replace::new(Duration::from_secs(2))),
                ..Default::default()
            })
        );
    }

    #[test]
    fn test_merge_output_logs_per_facet() {
        use omni_task_output_logs::{
            LogsDisplay, OutputLogsConfiguration, OutputLogsSplit,
        };

        let mut a = TaskConfiguration::long_form(TaskConfigurationLongForm {
            output_logs: Some(OutputLogsConfiguration::Uniform(
                LogsDisplay::Failed,
            )),
            ..Default::default()
        });

        let b = TaskConfiguration::long_form(TaskConfigurationLongForm {
            output_logs: Some(OutputLogsConfiguration::Split(
                OutputLogsSplit {
                    new: Some(LogsDisplay::All),
                    cached: None,
                },
            )),
            ..Default::default()
        });

        a.merge(b);

        assert_eq!(
            a.output_logs().unwrap().normalized(),
            (Some(LogsDisplay::All), Some(LogsDisplay::Failed))
        );
    }

    #[test]
    fn test_output_logs_short_form_is_none() {
        let short = TaskConfiguration::ShortForm(CommandConfig::Shell(
            "echo hi".to_string(),
        ));
        assert!(short.output_logs().is_none());
    }

    #[test]
    fn test_task_long_form_rejects_unknown_field() {
        let result = serde_json::from_str::<TaskConfigurationLongForm>(
            r#"{"exec": "echo hi", "bogus": 1}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_task_env_rejects_unknown_field() {
        let result = serde_json::from_str::<TaskEnvConfiguration>(
            r#"{"vars": {}, "bogus": 1}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_meta_configuration_accepts_arbitrary_keys() {
        // MetaConfiguration is a catch-all (#[serde(transparent)] over
        // DictConfig<DynValue>) and must accept arbitrary unknown keys.
        let result = serde_json::from_str::<MetaConfiguration>(
            r#"{"anything": 1, "custom_key": "value"}"#,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_long_form_deserializes_base_and_single_extends() {
        let task: TaskConfiguration = serde_json::from_str(
            r#"{"exec": "echo hi", "base": true, "extends": "other"}"#,
        )
        .unwrap();

        let TaskConfiguration::LongForm(long) = task else {
            panic!("should be long form");
        };
        assert!(long.base);
        assert_eq!(
            long.extends,
            Some(SingleOrMany::Single("other".to_string()))
        );
    }

    #[test]
    fn test_merge_extends_cleared_with_empty_list() {
        // An explicit empty list is the escape hatch to clear an inherited
        // `extends` (omission/null inherits instead).
        let mut a = TaskConfiguration::long_form(TaskConfigurationLongForm {
            extends: Some(SingleOrMany::Single("x".to_string())),
            ..Default::default()
        });

        let b = TaskConfiguration::long_form(TaskConfigurationLongForm {
            extends: Some(SingleOrMany::Many(vec![])),
            ..Default::default()
        });

        a.merge(b);

        let TaskConfiguration::LongForm(long) = a else {
            panic!("should be long form");
        };
        assert_eq!(long.extends, Some(SingleOrMany::Many(vec![])));
    }

    #[test]
    fn test_long_form_deserializes_extends_list() {
        let task: TaskConfiguration = serde_json::from_str(
            r#"{"exec": "echo hi", "extends": ["a", "b"]}"#,
        )
        .unwrap();

        let TaskConfiguration::LongForm(long) = task else {
            panic!("should be long form");
        };
        assert!(!long.base);
        assert_eq!(
            long.extends,
            Some(SingleOrMany::Many(vec!["a".to_string(), "b".to_string()]))
        );
    }

    #[test]
    fn test_long_form_defaults_base_false_extends_none() {
        let task: TaskConfiguration =
            serde_json::from_str(r#"{"exec": "echo hi"}"#).unwrap();

        let TaskConfiguration::LongForm(long) = task else {
            panic!("should be long form");
        };
        assert!(!long.base);
        assert_eq!(long.extends, None);
    }

    #[test]
    fn test_short_form_string_has_no_base_or_extends() {
        // A bare string stays short form and carries no base/extends: declaring
        // either requires the map (long) form.
        let task: TaskConfiguration =
            serde_json::from_str(r#""echo hi""#).unwrap();
        assert!(matches!(task, TaskConfiguration::ShortForm(_)));
    }

    #[test]
    fn test_long_form_base_extends_round_trip() {
        let original =
            TaskConfiguration::long_form(TaskConfigurationLongForm {
                base: true,
                extends: Some(SingleOrMany::Many(vec![
                    "a".to_string(),
                    "b".to_string(),
                ])),
                exec: Some(Replace::new(CommandConfig::Shell(
                    "echo hi".to_string(),
                ))),
                ..Default::default()
            });

        let json = serde_json::to_string(&original).unwrap();
        let round_tripped: TaskConfiguration =
            serde_json::from_str(&json).unwrap();

        assert_eq!(original, round_tripped);
    }

    #[test]
    fn test_merge_extends_kept_when_override_omits() {
        // `extends` must survive when the overriding layer does not specify it,
        // so partially overriding a task never silently drops its inherited
        // task extension.
        let mut a = TaskConfiguration::long_form(TaskConfigurationLongForm {
            base: true,
            extends: Some(SingleOrMany::Single("x".to_string())),
            ..Default::default()
        });

        let b = TaskConfiguration::long_form(TaskConfigurationLongForm {
            base: false,
            extends: None,
            ..Default::default()
        });

        a.merge(b);

        let TaskConfiguration::LongForm(long) = a else {
            panic!("should be long form");
        };
        // `base` is still replaced wholesale by the overriding layer.
        assert!(!long.base);
        // `extends` is retained because the override omitted it.
        assert_eq!(long.extends, Some(SingleOrMany::Single("x".to_string())));
    }

    #[test]
    fn test_merge_extends_replaced_when_override_present() {
        // When the override provides its own `extends`, it replaces the base's.
        let mut a = TaskConfiguration::long_form(TaskConfigurationLongForm {
            extends: Some(SingleOrMany::Single("x".to_string())),
            ..Default::default()
        });

        let b = TaskConfiguration::long_form(TaskConfigurationLongForm {
            extends: Some(SingleOrMany::Single("y".to_string())),
            ..Default::default()
        });

        a.merge(b);

        let TaskConfiguration::LongForm(long) = a else {
            panic!("should be long form");
        };
        assert_eq!(long.extends, Some(SingleOrMany::Single("y".to_string())));
    }
}
