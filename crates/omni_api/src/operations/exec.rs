use omni_context::LoadedContext;
use omni_execution_plan::Call;
use omni_messages::ExecutionEventSubscriber;
use omni_task_executor::{
    ExecutionConfigBuilder, TaskExecutionResult, TaskExecutor, TaskExecutorSys,
};
use omni_task_output_logs::LogsDisplay;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::operations::task::apply_filters;

use super::task::TaskRunFilters;

// ── Request ────────────────────────────────────────────────────────────────────

/// Request to run an arbitrary command in the workspace environment.
#[derive(Debug, Clone, JsonSchema)]
pub struct ExecRequest {
    /// The command and its arguments (e.g. `["echo", "hello"]`).
    pub cmd: Vec<String>,
    /// Filters that narrow down which projects are in scope.
    pub filters: TaskRunFilters,
    /// Output logs to display to the user.
    pub output_logs: Option<LogsDisplay>,
    pub output_cached_logs: Option<LogsDisplay>,
}

impl Default for ExecRequest {
    fn default() -> Self {
        Self {
            cmd: vec![],
            filters: TaskRunFilters::default(),
            output_logs: None,
            output_cached_logs: None,
        }
    }
}

// ── Response ──────────────────────────────────────────────────────────────────

/// Results of an `exec` operation.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ExecResponse {
    // `TaskExecutionResult` lives in `omni_task_executor` and does not
    // implement `JsonSchema`; represent it opaquely here.
    pub results: Vec<TaskExecutionResult>,
}

impl ExecResponse {
    pub fn is_success(&self) -> bool {
        !self.results.iter().any(|r| r.is_failure())
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Execute an arbitrary command in the workspace environment.
pub async fn handle_exec<TSys, S>(
    ctx: &LoadedContext<TSys>,
    subscriber: &S,
    req: ExecRequest,
) -> eyre::Result<ExecResponse>
where
    TSys: TaskExecutorSys + Clone,
    S: ExecutionEventSubscriber,
{
    if req.cmd.is_empty() {
        eyre::bail!(
            "no command provided to exec; pass at least one element in `cmd`"
        );
    }

    let mut builder = ExecutionConfigBuilder::default();

    builder.call(Call::new_command(req.cmd[0].clone(), req.cmd[1..].to_vec()));

    if let Some(output_logs) = req.output_logs {
        builder.output_logs(output_logs);
    }

    if let Some(output_logs) = req.output_cached_logs {
        builder.output_cached_logs(output_logs);
    }

    apply_filters(&mut builder, &req.filters);

    let config = builder.build()?;
    let executor = TaskExecutor::new(config, ctx, subscriber);
    let results = executor.run().await?;

    Ok(ExecResponse { results })
}
