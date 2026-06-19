use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunTasksParams {
    pub tasks: Vec<String>,
    #[serde(default)]
    pub project: Vec<String>,
    #[serde(default)]
    pub dir: Vec<String>,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub ignore_dependencies: bool,
    #[serde(default)]
    pub args: Vec<(String, String)>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunTasksResult {
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
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecCommandParams {
    pub cmd: Vec<String>,
    #[serde(default)]
    pub project: Vec<String>,
    #[serde(default)]
    pub dir: Vec<String>,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecCommandResult {
    pub ok: bool,
    pub results: Vec<TaskExecutionSummary>,
}
