use std::borrow::Cow;
use std::fmt::Display;
use std::str::FromStr;

use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::de::Error;
use serde::{Deserialize, Serialize};
use strum::EnumIs;

use omni_types::{ReferenceError, ReferenceSurface, TaskReference};

#[derive(Debug, Clone, PartialEq, Eq, EnumIs, Validate)]
#[garde(allow_unvalidated)]
pub enum TaskDependencyConfiguration {
    Own { task: String },
    ExplicitProject { project: String, task: String },
    Upstream { task: String },
}

impl FromStr for TaskDependencyConfiguration {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let reference = TaskReference::parse(s)
            .map_err(|e| eyre::eyre!("invalid task reference `{s}`: {e}"))?;
        Self::from_reference(&reference)
            .map_err(|e| eyre::eyre!("invalid task reference `{s}`: {e}"))
    }
}

impl TaskDependencyConfiguration {
    fn from_reference(
        reference: &TaskReference,
    ) -> Result<Self, ReferenceError> {
        reference.validate_for(ReferenceSurface::DependencyOrWith)?;
        if reference.plugin().is_some() {
            return Err(ReferenceError::NotAllowed {
                surface: ReferenceSurface::DependencyOrWith,
                reason: "be plugin-scoped yet; plugin scopes are not supported in this version (escape a literal `::` with `\\:\\:`)",
            });
        }
        let task = reference.task().to_string();
        Ok(match (reference.upstream(), reference.project()) {
            (true, _) => Self::Upstream { task },
            (false, Some(project)) => Self::ExplicitProject {
                project: project.to_string(),
                task,
            },
            (false, None) => Self::Own { task },
        })
    }

    fn to_reference(&self) -> Result<TaskReference, ReferenceError> {
        let (upstream, project, task) = match self {
            Self::Own { task } => (false, None, task),
            Self::ExplicitProject { project, task } => {
                (false, Some(project), task)
            }
            Self::Upstream { task } => (true, None, task),
        };
        TaskReference::from_raw_components(
            upstream,
            project.cloned(),
            None,
            task.clone(),
        )
    }
}

impl Display for TaskDependencyConfiguration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.to_reference() {
            Ok(reference) => reference.format().fmt(f),
            // Variants are public, so a value built by hand can be invalid.
            Err(_) => write!(f, "<invalid task reference {self:?}>"),
        }
    }
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
            TaskDependencyConfiguration::Own { task }
            | TaskDependencyConfiguration::ExplicitProject { task, .. }
            | TaskDependencyConfiguration::Upstream { task } => task,
        }
    }

    pub fn project(&self) -> Option<&str> {
        match self {
            TaskDependencyConfiguration::Own { .. }
            | TaskDependencyConfiguration::Upstream { .. } => None,
            TaskDependencyConfiguration::ExplicitProject {
                project, ..
            } => Some(project),
        }
    }
}

impl From<TaskDependencyConfiguration> for omni_core::TaskDependency {
    fn from(val: TaskDependencyConfiguration) -> Self {
        match val {
            TaskDependencyConfiguration::Own { task } => {
                omni_core::TaskDependency::Own { task }
            }
            TaskDependencyConfiguration::ExplicitProject { project, task } => {
                omni_core::TaskDependency::ExplicitProject { project, task }
            }
            TaskDependencyConfiguration::Upstream { task } => {
                omni_core::TaskDependency::Upstream { task }
            }
        }
    }
}

/// The structured form: raw components, never reparsed as shorthand.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct TaskDependencyObject {
    /// The project that owns the task. Omit for a task in the same project.
    project: Option<String>,
    /// The raw task name. Characters such as `#`, `::` and a leading `^` are
    /// literal here, not syntax.
    task: String,
}

impl<'de> Deserialize<'de> for TaskDependencyConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct V;

        impl<'de> serde::de::Visitor<'de> for V {
            type Value = TaskDependencyConfiguration;

            fn expecting(
                &self,
                f: &mut std::fmt::Formatter<'_>,
            ) -> std::fmt::Result {
                f.write_str(
                    "a task reference string or a `{ project?, task }` object",
                )
            }

            fn visit_str<E: Error>(self, s: &str) -> Result<Self::Value, E> {
                TaskDependencyConfiguration::from_str(s).map_err(E::custom)
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                let obj = TaskDependencyObject::deserialize(
                    serde::de::value::MapAccessDeserializer::new(map),
                )?;
                let reference = TaskReference::from_raw_components(
                    false,
                    obj.project,
                    None,
                    obj.task,
                )
                .map_err(A::Error::custom)?;
                TaskDependencyConfiguration::from_reference(&reference)
                    .map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_any(V)
    }
}

impl Serialize for TaskDependencyConfiguration {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let reference =
            self.to_reference().map_err(serde::ser::Error::custom)?;
        reference.format().serialize(s)
    }
}

impl JsonSchema for TaskDependencyConfiguration {
    fn schema_name() -> Cow<'static, str> {
        "TaskDependencyConfiguration".into()
    }

    fn json_schema(
        generator: &mut schemars::SchemaGenerator,
    ) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A task reference: a string (`task`, `project#task`, `^task`; escape `\\`, `#`, `:` and `^` with a backslash) or a `{ project?, task }` object whose values are literal.",
            "anyOf": [
                generator.subschema_for::<String>(),
                generator.subschema_for::<TaskDependencyObject>(),
            ]
        })
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

    fn parse(json: &str) -> Result<TaskDependencyConfiguration, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    #[test]
    fn object_form_keeps_raw_components() {
        let dc =
            parse(r#"{"project":"common","task":"native::build"}"#).unwrap();
        assert_eq!(
            dc,
            TaskDependencyConfiguration::ExplicitProject {
                project: "common".into(),
                task: "native::build".into()
            }
        );
        // The literal name serializes escaped, so it round-trips.
        let text = serde_json::to_string(&dc).unwrap();
        assert_eq!(text, r#""common#native\\:\\:build""#);
        assert_eq!(parse(&text).unwrap(), dc);

        let own = parse(r#"{"task":"^x"}"#).unwrap();
        assert_eq!(own, TaskDependencyConfiguration::Own { task: "^x".into() });
        assert_eq!(parse(&serde_json::to_string(&own).unwrap()).unwrap(), own);
    }

    #[test]
    fn object_form_is_strict() {
        assert!(parse(r#"{"task":"a","extra":1}"#).is_err());
        assert!(parse(r#"{"task":"a","task":"b"}"#).is_err());
        assert!(parse(r#"{"project":"p"}"#).is_err());
        assert!(parse(r#"{"project":"","task":"a"}"#).is_err());
        assert!(parse(r#"{"task":""}"#).is_err());
    }

    #[test]
    fn strings_are_parsed_strictly() {
        // The old unanchored regex silently truncated this to `a`.
        assert_eq!(
            parse(r#""a b""#).unwrap(),
            TaskDependencyConfiguration::Own { task: "a b".into() }
        );
        assert!(parse(r#""a#b#c""#).is_err());
        assert!(parse(r#""^p#t""#).is_err());
        assert!(parse(r#""""#).is_err());
        assert_eq!(
            parse(r#""lint:fix""#).unwrap(),
            TaskDependencyConfiguration::Own {
                task: "lint:fix".into()
            }
        );
    }

    #[test]
    fn plugin_scopes_are_rejected_until_supported() {
        let err = parse(r#""native::build""#).unwrap_err();
        assert!(err.contains("plugin"), "{err}");
        // The escaped form is the literal task name.
        assert_eq!(
            parse(r#""native\\:\\:build""#).unwrap(),
            TaskDependencyConfiguration::Own {
                task: "native::build".into()
            }
        );
    }

    #[test]
    fn hand_built_invalid_values_do_not_panic() {
        let bad = TaskDependencyConfiguration::Own {
            task: String::new(),
        };
        assert!(bad.to_string().contains("invalid"));
        assert!(serde_json::to_string(&bad).is_err());
    }

    #[test]
    fn schema_accepts_string_or_object() {
        let schema = schemars::schema_for!(TaskDependencyConfiguration);
        let v = serde_json::to_value(&schema).unwrap();
        assert_eq!(v["anyOf"].as_array().unwrap().len(), 2);
    }
}
