use omni_task_output_logs::LogsDisplay;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskRunParams {
    /// Tasks to run.
    pub tasks: Vec<String>,
    /// Project filter where tasks will be executed.
    #[serde(default)]
    pub project: Vec<String>,
    /// Directory filter where tasks will be executed.
    #[serde(default)]
    pub dir: Vec<String>,
    /// Dry run mode.
    #[serde(default)]
    pub dry_run: bool,
    /// Force execution of tasks, ignoring cached results.
    #[serde(default)]
    pub force: bool,
    /// Ignore dependencies and run tasks independently.
    #[serde(default)]
    pub ignore_dependencies: bool,
    #[serde(default)]
    pub args: Vec<(String, String)>,
    /// Include logs in the response.
    #[serde(default)]
    pub include_logs: Option<LogsDisplay>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskRunResult {
    pub ok: bool,
    pub results: Vec<TaskExecutionSummary>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskExecutionSummary {
    pub project: String,
    pub task: String,
    pub status: String,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<u32>,
    pub logs: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecCommandParams {
    /// Command to execute
    pub cmd: Vec<String>,
    /// Project filter where command will be executed
    #[serde(default)]
    pub project: Vec<String>,
    /// Directory filter where command will be executed.
    #[serde(default)]
    pub dir: Vec<String>,
    /// Dry run mode.
    #[serde(default)]
    pub dry_run: bool,
    /// Include logs in the response.
    #[serde(default)]
    pub include_logs: Option<LogsDisplay>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecCommandResult {
    pub ok: bool,
    pub results: Vec<TaskExecutionSummary>,
}
