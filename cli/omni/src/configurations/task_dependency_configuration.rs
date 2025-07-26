use std::borrow::Cow;

use lazy_regex::{Lazy, Regex, regex};
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::EnumIs;

#[derive(Debug, Clone, PartialEq, Eq, EnumIs)]
pub enum TaskDependencyConfiguration {
    Own { task: String },
    ExplicitProject { project: String, task: String },
    Upstream { task: String },
}

impl Merge for TaskDependencyConfiguration {
    #[inline(always)]
    fn merge(&mut self, other: Self) {
        *self = other;
    }
}

impl TaskDependencyConfiguration {
    pub fn task(&self) -> &str {
        match self {
            TaskDependencyConfiguration::Own { task } => task,
            TaskDependencyConfiguration::ExplicitProject { task, .. } => task,
            TaskDependencyConfiguration::Upstream { task } => task,
        }
    }

    pub fn project(&self) -> Option<&str> {
        match self {
            TaskDependencyConfiguration::Own { .. } => None,
            TaskDependencyConfiguration::ExplicitProject {
                project, ..
            } => Some(project),
            TaskDependencyConfiguration::Upstream { .. } => None,
        }
    }
}

static TASK_REGEX: &Lazy<Regex> = regex!(
    r#"((?<explicit_project>[/\.\@\:\w\-]+)#(?<explicit_task>[/\.\@\:\w\-]+))|(\^(?<upstream_task>[/\.\@\:\w-]+))|(?<own_task>[/\.\@\w\-\:]+)"#
);

impl From<TaskDependencyConfiguration> for crate::core::TaskDependency {
    fn from(val: TaskDependencyConfiguration) -> Self {
        match val {
            TaskDependencyConfiguration::Own { task } => {
                crate::core::TaskDependency::Own { task }
            }
            TaskDependencyConfiguration::ExplicitProject { project, task } => {
                crate::core::TaskDependency::ExplicitProject { project, task }
            }
            TaskDependencyConfiguration::Upstream { task } => {
                crate::core::TaskDependency::Upstream { task }
            }
        }
    }
}

impl<'de> Deserialize<'de> for TaskDependencyConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        if TASK_REGEX.is_match(&s) {
            let captures =
                TASK_REGEX.captures(&s).expect("Can't parse task syntax");

            if let Some(upstream_task) = captures.name("upstream_task") {
                return Ok(Self::Upstream {
                    task: upstream_task.as_str().to_string(),
                });
            }

            if let Some(own_task) = captures.name("own_task") {
                return Ok(Self::Own {
                    task: own_task.as_str().to_string(),
                });
            }

            if let (Some(explicit_project), Some(explicit_task)) = (
                captures.name("explicit_project"),
                captures.name("explicit_task"),
            ) {
                return Ok(Self::ExplicitProject {
                    project: explicit_project.as_str().to_string(),
                    task: explicit_task.as_str().to_string(),
                });
            }
        }

        panic!("Can't parse TaskDependencyConfiguration: {s}");
    }
}

impl Serialize for TaskDependencyConfiguration {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            TaskDependencyConfiguration::Own { task } => task.serialize(s),
            TaskDependencyConfiguration::ExplicitProject { project, task } => {
                format!("{project}#{task}").serialize(s)
            }
            TaskDependencyConfiguration::Upstream { task } => {
                format!("^{task}").serialize(s)
            }
        }
    }
}

impl JsonSchema for TaskDependencyConfiguration {
    fn schema_name() -> Cow<'static, str> {
        String::schema_name()
    }

    fn json_schema(
        generator: &mut schemars::SchemaGenerator,
    ) -> schemars::Schema {
        String::json_schema(generator)
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn test_deserialize_own() {
        let dc: TaskDependencyConfiguration = serde_json::from_str("\"task\"")
            .expect("Can't parse DependencyConfiguration");

        assert!(dc.is_own(), "Should be own");
        assert_eq!(dc.task(), "task");
    }

    #[test]
    fn test_deserialize_explicit_project() {
        let dc: TaskDependencyConfiguration =
            serde_json::from_str("\"project#task\"")
                .expect("Can't parse DependencyConfiguration");

        assert!(dc.is_explicit_project(), "Should be explicit");
        assert_eq!(dc.project().unwrap(), "project");
        assert_eq!(dc.task(), "task");
    }

    #[test]
    fn test_deserialize_upstream() {
        let dc: TaskDependencyConfiguration = serde_json::from_str("\"^task\"")
            .expect("Can't parse DependencyConfiguration");

        assert!(dc.is_upstream(), "Should be upstream");
        assert_eq!(dc.task(), "task");
    }

    #[test]
    fn test_serialize_own() {
        let dc = TaskDependencyConfiguration::Own {
            task: "task".to_string(),
        };

        let serialized = serde_json::to_string(&dc)
            .expect("Can't serialize DependencyConfiguration");

        assert_eq!(serialized, "\"task\"");
    }

    #[test]
    fn test_serialize_explicit_project() {
        let dc = TaskDependencyConfiguration::ExplicitProject {
            project: "project".to_string(),
            task: "task".to_string(),
        };

        let serialized = serde_json::to_string(&dc)
            .expect("Can't serialize DependencyConfiguration");

        assert_eq!(serialized, "\"project#task\"");
    }

    #[test]
    fn test_serialize_upstream() {
        let dc = TaskDependencyConfiguration::Upstream {
            task: "task".to_string(),
        };

        let serialized = serde_json::to_string(&dc)
            .expect("Can't serialize DependencyConfiguration");

        assert_eq!(serialized, "\"^task\"");
    }

    #[test]
    fn test_deserialize_special_chars() {
        const PROJECT_NAME: &str = "@repo/project.name-with_@special-chars";
        const TASK_NAME: &str = "@task:name";

        let dc: TaskDependencyConfiguration =
            serde_json::from_str(&format!("\"{PROJECT_NAME}#{TASK_NAME}\""))
                .expect("Can't parse DependencyConfiguration");

        assert!(dc.is_explicit_project(), "Should be explicit");
        assert_eq!(dc.project().unwrap(), PROJECT_NAME);
        assert_eq!(dc.task(), TASK_NAME);
    }
}
