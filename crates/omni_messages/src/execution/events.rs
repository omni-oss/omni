use std::time::Duration;

use serde::{Deserialize, Serialize};

// ─── Task lifecycle ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStartedEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    /// The command that would run. Set only in dry runs, already redacted
    /// (RFC 0022), so presenters can show it without running anything.
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletedEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    pub exit_code: u32,
    pub elapsed: Duration,
    pub cache_hit: bool,
    pub tries: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFailedEvent {
    pub task_id: String,
    pub project: String,
    pub task: String,
    pub error: String,
    pub tries: u16,
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
    pub attempt: u16,
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
    /// No command execution, cache recording or cache replay will happen.
    #[serde(default)]
    pub dry_run: bool,
    /// A remote cache is configured for this run.
    #[serde(default)]
    pub remote_cache: bool,
    /// Redaction mode in effect for this run.
    #[serde(default)]
    pub redact_mode: PlanRedactMode,
}

/// Redaction mode in effect for a run (RFC 0022), as presenters see it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum PlanRedactMode {
    #[default]
    Declared,
    All,
    None,
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
