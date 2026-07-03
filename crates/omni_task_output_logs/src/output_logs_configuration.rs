use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};
use serde_untagged::UntaggedEnumVisitor;

use crate::logs_display::{LogsDisplay, logs_display_from_str};

fn replace_if_some<T>(left: &mut Option<T>, right: Option<T>) {
    if right.is_some() {
        *left = right;
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    JsonSchema,
    Merge,
    Default,
)]
pub struct OutputLogsSplit {
    /// Fresh (cache-miss) output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = replace_if_some)]
    pub new: Option<LogsDisplay>,
    /// Replayed cached (cache-hit) output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[merge(strategy = replace_if_some)]
    pub cached: Option<LogsDisplay>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[serde(untagged)]
pub enum OutputLogsConfiguration {
    /// `output_logs: failed` — applies to both facets.
    Uniform(LogsDisplay),
    /// `output_logs: { new: all, cached: never }` — per facet.
    Split(OutputLogsSplit),
}

impl OutputLogsConfiguration {
    /// Converts to the canonical split form, expanding a `Uniform` value to
    /// both facets.
    pub fn to_split(&self) -> OutputLogsSplit {
        match self {
            OutputLogsConfiguration::Uniform(display) => OutputLogsSplit {
                new: Some(*display),
                cached: Some(*display),
            },
            OutputLogsConfiguration::Split(split) => *split,
        }
    }

    /// Returns the normalised `(new, cached)` pair that merges across levels
    /// and flows to the executor.
    pub fn normalized(&self) -> (Option<LogsDisplay>, Option<LogsDisplay>) {
        let split = self.to_split();
        (split.new, split.cached)
    }
}

impl Serialize for OutputLogsConfiguration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            OutputLogsConfiguration::Uniform(display) => {
                display.serialize(serializer)
            }
            OutputLogsConfiguration::Split(split) => {
                split.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for OutputLogsConfiguration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        UntaggedEnumVisitor::new()
            .string(|s| {
                logs_display_from_str(s).map(OutputLogsConfiguration::Uniform)
            })
            .map(|map| map.deserialize().map(OutputLogsConfiguration::Split))
            .deserialize(deserializer)
    }
}

impl Merge for OutputLogsConfiguration {
    fn merge(&mut self, other: Self) {
        let mut base = self.to_split();
        base.merge(other.to_split());
        *self = OutputLogsConfiguration::Split(base);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_scalar() {
        let value: OutputLogsConfiguration =
            serde_json::from_str("\"failed\"").unwrap();
        assert_eq!(
            value,
            OutputLogsConfiguration::Uniform(LogsDisplay::Failed)
        );
    }

    #[test]
    fn deserialize_split() {
        let value: OutputLogsConfiguration =
            serde_json::from_str(r#"{"new":"all","cached":"never"}"#).unwrap();
        assert_eq!(
            value,
            OutputLogsConfiguration::Split(OutputLogsSplit {
                new: Some(LogsDisplay::All),
                cached: Some(LogsDisplay::Never),
            })
        );
    }

    #[test]
    fn deserialize_partial_split() {
        let value: OutputLogsConfiguration =
            serde_json::from_str(r#"{"new":"all"}"#).unwrap();
        assert_eq!(
            value,
            OutputLogsConfiguration::Split(OutputLogsSplit {
                new: Some(LogsDisplay::All),
                cached: None,
            })
        );
    }

    #[test]
    fn deserialize_invalid_scalar_reports_variant() {
        let err = serde_json::from_str::<OutputLogsConfiguration>("\"nope\"")
            .unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn serialize_scalar() {
        let value = OutputLogsConfiguration::Uniform(LogsDisplay::Failed);
        assert_eq!(serde_json::to_string(&value).unwrap(), "\"failed\"");
    }

    #[test]
    fn serialize_split() {
        let value = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: Some(LogsDisplay::All),
            cached: Some(LogsDisplay::Never),
        });
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#"{"new":"all","cached":"never"}"#
        );
    }

    #[test]
    fn normalized_expands_uniform() {
        let value = OutputLogsConfiguration::Uniform(LogsDisplay::All);
        assert_eq!(
            value.normalized(),
            (Some(LogsDisplay::All), Some(LogsDisplay::All))
        );
    }

    #[test]
    fn normalized_passes_split_through() {
        let value = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: Some(LogsDisplay::All),
            cached: None,
        });
        assert_eq!(value.normalized(), (Some(LogsDisplay::All), None));
    }

    #[test]
    fn merge_task_overrides_project_per_facet() {
        // project (base): uniform failed -> {new: failed, cached: failed}
        let mut project = OutputLogsConfiguration::Uniform(LogsDisplay::Failed);
        // task overrides only `new`
        let task = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: Some(LogsDisplay::All),
            cached: None,
        });

        project.merge(task);

        assert_eq!(
            project.normalized(),
            (Some(LogsDisplay::All), Some(LogsDisplay::Failed))
        );
    }

    #[test]
    fn merge_unset_facet_keeps_base() {
        let mut project = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: Some(LogsDisplay::Never),
            cached: Some(LogsDisplay::All),
        });
        let task = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: None,
            cached: Some(LogsDisplay::Failed),
        });

        project.merge(task);

        assert_eq!(
            project.normalized(),
            (Some(LogsDisplay::Never), Some(LogsDisplay::Failed))
        );
    }
}
