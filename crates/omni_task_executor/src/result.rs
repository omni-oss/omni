use derive_new::new;
use omni_configurations::MetaConfiguration;
use omni_core::TaskExecutionNode;
use omni_hasher::impls::DefaultHash;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIs};

#[derive(Debug, new, EnumIs, Serialize, Deserialize)]
#[serde(tag = "status")]
#[serde(rename_all = "kebab-case")]
pub enum TaskExecutionResult {
    Completed {
        #[serde(with = "crate::serde_impls::default_hash_to_string")]
        hash: DefaultHash,
        task: TaskExecutionNode,
        exit_code: u32,
        elapsed: std::time::Duration,
        cache_hit: bool,
        #[new(default)]
        details: Option<TaskDetails>,
        tries: u8,
    },
    Errored {
        task: TaskExecutionNode,
        error: String,
        #[new(default)]
        details: Option<TaskDetails>,
        tries: u8,
    },
    Skipped {
        task: TaskExecutionNode,
        skip_reason: SkipReason,
        #[new(default)]
        details: Option<TaskDetails>,
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

#[derive(Debug, new, EnumIs, Display, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipReason {
    #[strum(to_string = "task in a previous batch failed")]
    PreviousBatchFailure,
    #[strum(to_string = "dependee task failed")]
    DependeeTaskFailure,
    #[strum(to_string = "task is disabled")]
    Disabled,
}

#[derive(Debug, Clone, new, Serialize, Deserialize, Default)]
pub struct TaskDetails {
    pub meta: Option<MetaConfiguration>,
}
