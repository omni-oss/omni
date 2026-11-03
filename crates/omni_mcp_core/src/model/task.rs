use omni_task_executor::{Force, MetaOnError};
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
    /// CEL expression selecting tasks by their `meta` configuration. It must
    /// evaluate to a bool; use `has(meta.key)` for keys that may be missing.
    #[serde(default)]
    pub meta: Option<String>,
    /// What to do when the meta filter fails for a task: `fail` (default)
    /// returns an error describing the failures, `exclude` skips those tasks.
    #[serde(default)]
    pub meta_on_error: MetaOnError,
    /// Failing tasks to list per distinct meta error cause (default 3).
    #[serde(default = "default_meta_error_samples")]
    pub meta_error_samples: usize,
    /// Dry run mode.
    #[serde(default)]
    pub dry_run: bool,
    /// Force execution of tasks: `all` re-runs every selected task ignoring
    /// cached results, `failed` re-runs only tasks that failed last time, and
    /// `none` (default) uses the cache normally.
    #[serde(default)]
    pub force: Force,
    /// Ignore dependencies and run tasks independently.
    #[serde(default)]
    pub ignore_dependencies: bool,
    #[serde(default)]
    pub args: Vec<(String, String)>,
    /// Include logs in the response.
    #[serde(default)]
    pub include_logs: Option<LogsDisplay>,
}

fn default_meta_error_samples() -> usize {
    3
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse(value: serde_json::Value) -> Result<TaskRunParams, String> {
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    #[test]
    fn force_defaults_to_none() {
        let params = parse(json!({ "tasks": ["test"] })).unwrap();
        assert_eq!(params.force, Force::None);
    }

    #[test]
    fn force_accepts_cli_values() {
        for (value, expected) in [
            ("all", Force::All),
            ("failed", Force::Failed),
            ("none", Force::None),
        ] {
            let params =
                parse(json!({ "tasks": ["test"], "force": value })).unwrap();
            assert_eq!(params.force, expected, "{value}");
        }
    }

    #[test]
    fn force_rejects_booleans_and_unknown_values() {
        for value in [json!(true), json!(false), json!("bogus")] {
            parse(json!({ "tasks": ["test"], "force": value.clone() }))
                .expect_err(&format!("{value} should be rejected"));
        }
    }
}
