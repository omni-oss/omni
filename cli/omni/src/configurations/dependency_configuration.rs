use std::borrow::Cow;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyConfiguration {
    pub project: String,
    pub task: String,
}

impl From<DependencyConfiguration> for crate::core::Dependency {
    fn from(val: DependencyConfiguration) -> Self {
        Self {
            project: if val.project.is_empty() {
                None
            } else {
                Some(val.project)
            },
            task: if val.task.is_empty() {
                None
            } else {
                Some(val.task)
            },
        }
    }
}

impl<'de> Deserialize<'de> for DependencyConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        if s.contains("#") {
            let mut split = s.split("#");
            let project = split.next().expect("Can't get project").to_string();
            let task = split.next().expect("Can't get task").to_string();

            Ok(DependencyConfiguration { project, task })
        } else {
            Ok(DependencyConfiguration {
                project: s,
                task: String::new(),
            })
        }
    }
}

impl Serialize for DependencyConfiguration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.task.is_empty() {
            self.project.serialize(serializer)
        } else {
            format!("{}#{}", self.project, self.task).serialize(serializer)
        }
    }
}

impl JsonSchema for DependencyConfiguration {
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
    fn test_deserialize_dependency_configuration() {
        let dc: DependencyConfiguration =
            serde_json::from_str("\"project#task\"")
                .expect("Can't parse DependencyConfiguration");

        assert_eq!(dc.project, "project");
        assert_eq!(dc.task, "task");
    }

    #[test]
    fn test_serialize_dependency_configuration() {
        let dc = DependencyConfiguration {
            project: "project".to_string(),
            task: "task".to_string(),
        };

        let serialized = serde_json::to_string(&dc)
            .expect("Can't serialize DependencyConfiguration");

        assert_eq!(serialized, "\"project#task\"");
    }
}
