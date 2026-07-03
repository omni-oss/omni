use std::time::Duration;

use serde::{Deserialize, Serialize};

// ─── Task lifecycle ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStartedEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletedEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    pub exit_code: u32,
    pub elapsed: Duration,
    pub cache_hit: bool,
    pub tries: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFailedEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    pub error: String,
    pub tries: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSkippedEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    pub reason: TaskSkipReason,
    /// The full task name of the dependency that caused this skip.
    /// Set when `reason == DependeeTaskFailure`; `None` otherwise.
    pub dependency: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRetryingEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    pub attempt: u8,
    pub max_retries: u8,
    pub delay: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheHitEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    pub digest: Vec<u8>,
    /// `true` if the subscriber will also receive `on_task_output_stream` with
    /// `is_replay = true` for this task's cached logs; `false` if log replay
    /// was disabled by the resolved `cached` output-logs policy
    /// (e.g. `--output-cached-logs never`).
    pub replay_logs: bool,
    pub has_logs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanReadyEvent {
    /// Number of tasks scheduled for execution.
    pub total: usize,
    /// Whether any task in the plan is `interactive` or `persistent`.
    /// Subscribers may use this to select an appropriate output presenter
    /// (e.g. upgrade from stream to TUI mode before the first task starts).
    pub has_interactive_or_persistent_tasks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionCompleteEvent {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub cache_hits: usize,
    pub elapsed: Duration,
    pub total_time_saved: Duration,
}

// ─── Skip reason ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, strum::Display)]
pub enum TaskSkipReason {
    #[strum(to_string = "task in a previous batch failed")]
    PreviousBatchFailure,
    #[strum(to_string = "dependee task failed")]
    DependeeTaskFailure,
    #[strum(to_string = "task is disabled")]
    Disabled,
    #[strum(to_string = "no command to execute")]
    NoCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchStartEvent {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCompletedEvent {}
