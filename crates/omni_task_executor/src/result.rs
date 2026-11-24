use std::path::PathBuf;

use derive_new::new;
use omni_configurations::MetaConfiguration;
use omni_core::{TaskExecutionNode, TaskExecutionNodeReport};
use omni_hasher::impls::DefaultHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIs};

use crate::CacheStatus;

/// The outcome of an executed task. In-memory results carry an executable node;
/// read a serialized result back as a [`TaskExecutionResultReport`].
#[derive(Debug, new, EnumIs, Serialize, JsonSchema)]
#[serde(tag = "status")]
#[serde(rename_all = "kebab-case")]
pub enum TaskExecutionResult {
    Completed {
        #[serde(with = "crate::serde_impls::default_hash_to_string")]
        #[schemars(with = "String")]
        hash: DefaultHash,
        task: TaskExecutionNode,
        exit_code: u32,
        elapsed: std::time::Duration,
        cache_hit: bool,
        #[new(default)]
        details: Option<TaskDetails>,
        tries: u16,
        #[new(default)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_status: Option<CacheStatus>,
    },
    Errored {
        task: TaskExecutionNode,
        error: String,
        #[new(default)]
        details: Option<TaskDetails>,
        tries: u16,
        #[new(default)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_status: Option<CacheStatus>,
    },
    Skipped {
        task: TaskExecutionNode,
        skip_reason: SkipReason,
        #[new(default)]
        details: Option<TaskDetails>,
        #[new(default)]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_status: Option<CacheStatus>,
    },
}

impl TaskExecutionResult {
    pub fn success(&self) -> bool {
        matches!(self,
            TaskExecutionResult::Completed {exit_code, ..} if *exit_code == 0
        )
    }

    pub fn hash(&self) -> Option<DefaultHash> {
        match self {
            TaskExecutionResult::Completed { hash, .. } => Some(*hash),
            TaskExecutionResult::Errored { .. } => None,
            TaskExecutionResult::Skipped { .. } => None,
        }
    }

    pub fn is_skipped_due_to_error(&self) -> bool {
        matches!(
            self,
            TaskExecutionResult::Skipped {
                skip_reason: SkipReason::DependeeTaskFailure
                    | SkipReason::PreviousBatchFailure,
                ..
            }
        )
    }

    pub fn is_failure(&self) -> bool {
        self.is_skipped_due_to_error()
            || self.is_errored()
            || (self.is_completed() && !self.success())
    }

    pub fn task(&self) -> &TaskExecutionNode {
        match self {
            TaskExecutionResult::Completed { task, .. } => task,
            TaskExecutionResult::Errored { task, .. } => task,
            TaskExecutionResult::Skipped { task, .. } => task,
        }
    }

    pub fn details(&self) -> Option<&TaskDetails> {
        match self {
            TaskExecutionResult::Completed { details, .. }
            | TaskExecutionResult::Errored { details, .. }
            | TaskExecutionResult::Skipped { details, .. } => details.as_ref(),
        }
    }

    pub fn details_mut(&mut self) -> Option<&mut TaskDetails> {
        match self {
            TaskExecutionResult::Completed { details, .. }
            | TaskExecutionResult::Errored { details, .. }
            | TaskExecutionResult::Skipped { details, .. } => details.as_mut(),
        }
    }

    pub fn cache_status(&self) -> Option<&CacheStatus> {
        match self {
            TaskExecutionResult::Completed { cache_status, .. }
            | TaskExecutionResult::Errored { cache_status, .. }
            | TaskExecutionResult::Skipped { cache_status, .. } => {
                cache_status.as_ref()
            }
        }
    }

    pub fn set_cache_status(&mut self, status: CacheStatus) {
        match self {
            TaskExecutionResult::Completed { cache_status, .. }
            | TaskExecutionResult::Errored { cache_status, .. }
            | TaskExecutionResult::Skipped { cache_status, .. } => {
                *cache_status = Some(status);
            }
        }
    }

    pub fn set_details(&mut self, td: TaskDetails) {
        match self {
            TaskExecutionResult::Completed { details, .. }
            | TaskExecutionResult::Errored { details, .. }
            | TaskExecutionResult::Skipped { details, .. } => {
                *details = Some(td);
            }
        }
    }
}

/// A deserialized task result: a report, never an execution input. It carries a
/// [`TaskExecutionNodeReport`], whose labels are display text and are never
/// parsed back into an identity.
#[derive(Debug, EnumIs, Serialize, Deserialize)]
#[serde(tag = "status")]
#[serde(rename_all = "kebab-case")]
pub enum TaskExecutionResultReport {
    Completed {
        #[serde(with = "crate::serde_impls::default_hash_to_string")]
        hash: DefaultHash,
        task: TaskExecutionNodeReport,
        exit_code: u32,
        elapsed: std::time::Duration,
        cache_hit: bool,
        details: Option<TaskDetails>,
        tries: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_status: Option<CacheStatus>,
    },
    Errored {
        task: TaskExecutionNodeReport,
        error: String,
        details: Option<TaskDetails>,
        tries: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_status: Option<CacheStatus>,
    },
    Skipped {
        task: TaskExecutionNodeReport,
        skip_reason: SkipReason,
        details: Option<TaskDetails>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache_status: Option<CacheStatus>,
    },
}

impl TaskExecutionResultReport {
    pub fn task(&self) -> &TaskExecutionNodeReport {
        match self {
            Self::Completed { task, .. }
            | Self::Errored { task, .. }
            | Self::Skipped { task, .. } => task,
        }
    }
}

#[derive(Debug, new, EnumIs, Display, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    #[strum(to_string = "task in a previous batch failed")]
    PreviousBatchFailure,
    #[strum(to_string = "dependee task failed")]
    DependeeTaskFailure,
    #[strum(to_string = "task is disabled")]
    Disabled,
    #[strum(to_string = "no command to execute")]
    NoCommand,
}

#[derive(Debug, Clone, new, Serialize, Deserialize, Default, JsonSchema)]
pub struct TaskDetails {
    pub meta: Option<MetaConfiguration>,
    pub output_files: Option<Vec<PathBuf>>,
    pub cache_key_input_files: Option<Vec<PathBuf>>,
}

#[cfg(test)]
mod tests {
    use omni_core::{TaskExecutionNode, TaskKey};

    use super::*;

    fn node(
        project: &str,
        task: &str,
        deps: &[(&str, &str)],
    ) -> TaskExecutionNode {
        TaskExecutionNode::new(
            TaskKey::new(project, task),
            None,
            None,
            ".",
            deps.iter().map(|(p, t)| TaskKey::new(*p, *t)).collect(),
            true.into(),
            false,
            false,
            Default::default(),
        )
    }

    fn round_trip(result: &TaskExecutionResult) -> TaskExecutionResultReport {
        let value = serde_json::to_value(result).unwrap();
        let report: TaskExecutionResultReport =
            serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&report).unwrap(), value);
        report
    }

    #[test]
    fn completed_errored_and_skipped_results_read_back_as_reports() {
        let n = node("a#b", "c", &[("a", "b#c")]);
        let completed = TaskExecutionResult::new_completed(
            [7; 32],
            n.clone(),
            0,
            std::time::Duration::from_millis(5),
            true,
            1,
        );
        let errored =
            TaskExecutionResult::new_errored(n.clone(), "boom".into(), 2);
        let skipped = TaskExecutionResult::new_skipped(
            n.clone(),
            SkipReason::DependeeTaskFailure,
        );
        for result in [&completed, &errored, &skipped] {
            let report = round_trip(result);
            assert_eq!(report.task().project_name, "a#b");
            assert_eq!(report.task().task_name, "c");
            assert_eq!(report.task().full_task_name, r"a\#b#c");
            assert_eq!(report.task().dependencies, vec![r"a#b\#c".to_string()]);
        }
    }

    #[test]
    fn formerly_colliding_results_stay_distinct_in_reports() {
        let first = round_trip(&TaskExecutionResult::new_errored(
            node("a#b", "c", &[]),
            "x".into(),
            1,
        ));
        let second = round_trip(&TaskExecutionResult::new_errored(
            node("a", "b#c", &[]),
            "x".into(),
            1,
        ));
        assert_ne!(first.task().full_task_name, second.task().full_task_name);
    }

    #[test]
    fn results_with_a_cache_status_round_trip() {
        let statuses = [
            CacheStatus::Disabled {
                deciding_source: Some("task-explicit".into()),
            },
            CacheStatus::Demoted {
                direct_dependency: r"a#b\#c".into(),
                root_cause: "a#b".into(),
            },
        ];
        for status in statuses {
            let mut completed = TaskExecutionResult::new_completed(
                [1; 32],
                node("a#b", "c", &[]),
                0,
                std::time::Duration::from_millis(1),
                false,
                1,
            );
            completed.set_cache_status(status.clone());
            let mut skipped = TaskExecutionResult::new_skipped(
                node("a", "b#c", &[]),
                SkipReason::Disabled,
            );
            skipped.set_cache_status(status.clone());
            for result in [&completed, &skipped] {
                let value = serde_json::to_value(result).unwrap();
                assert!(value.get("cache_status").is_some());
                let report = round_trip(result);
                let got = match &report {
                    TaskExecutionResultReport::Completed {
                        cache_status,
                        ..
                    }
                    | TaskExecutionResultReport::Errored {
                        cache_status, ..
                    }
                    | TaskExecutionResultReport::Skipped {
                        cache_status, ..
                    } => cache_status.clone(),
                };
                assert_eq!(got, Some(status.clone()));
            }
        }
    }

    #[test]
    fn a_node_with_zero_dependencies_stays_executable() {
        let result = TaskExecutionResult::new_skipped(
            node("p", "t", &[]),
            SkipReason::NoCommand,
        );
        assert!(result.task().dependencies().is_empty());
    }
}
