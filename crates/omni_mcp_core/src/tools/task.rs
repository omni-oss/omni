use omni_api::{ExecRequest, RunFilters, RunRequest};
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_messages::OmniEventSubscriber;
use omni_task_executor::{
    Force, OnFailure, TaskExecutionResult, TaskExecutorSys,
};

use crate::{
    model::{
        ExecCommandParams, ExecCommandResult, RunTasksParams, RunTasksResult,
        TaskExecutionSummary,
    },
    server::OmniMcpServer,
};

impl<TSys, S> OmniMcpServer<TSys, S>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
    S: OmniEventSubscriber + Send + Sync + 'static,
{
    pub(crate) async fn tool_run_tasks(
        &self,
        params: RunTasksParams,
    ) -> eyre::Result<RunTasksResult> {
        let filters = RunFilters {
            project: params.project,
            dir: params.dir,
            dry_run: params.dry_run,
            args: params.args,
            ..Default::default()
        };
        let req = RunRequest {
            tasks: params.tasks,
            ignore_dependencies: params.ignore_dependencies,
            force: if params.force {
                Force::All
            } else {
                Force::None
            },
            filters,
            with_dependents: false,
            on_failure: OnFailure::SkipDependents,
            no_cache: false,
            output_logs: None,
            output_cached_logs: None,
        };
        let response = self.make_api().run(req).await?;
        let ok = response.is_success();
        let results =
            response.results.into_iter().map(summarize_result).collect();
        Ok(RunTasksResult { ok, results })
    }

    pub(crate) async fn tool_exec_command(
        &self,
        params: ExecCommandParams,
    ) -> eyre::Result<ExecCommandResult> {
        let filters = RunFilters {
            project: params.project,
            dir: params.dir,
            dry_run: params.dry_run,
            ..Default::default()
        };
        let req = ExecRequest {
            cmd: params.cmd,
            filters,
        };
        let response = self.make_api().exec(req).await?;
        let ok = response.is_success();
        let results =
            response.results.into_iter().map(summarize_result).collect();
        Ok(ExecCommandResult { ok, results })
    }
}

fn summarize_result(result: TaskExecutionResult) -> TaskExecutionSummary {
    match result {
        TaskExecutionResult::Completed {
            task,
            exit_code,
            elapsed,
            ..
        } => TaskExecutionSummary {
            project: task.project_name().to_string(),
            task: task.task_name().to_string(),
            status: "completed".to_string(),
            duration_ms: Some(elapsed.as_millis() as u64),
            exit_code: Some(exit_code),
        },
        TaskExecutionResult::Errored { task, .. } => TaskExecutionSummary {
            project: task.project_name().to_string(),
            task: task.task_name().to_string(),
            status: "errored".to_string(),
            duration_ms: None,
            exit_code: None,
        },
        TaskExecutionResult::Skipped {
            task, skip_reason, ..
        } => TaskExecutionSummary {
            project: task.project_name().to_string(),
            task: task.task_name().to_string(),
            status: format!("skipped:{skip_reason:?}"),
            duration_ms: None,
            exit_code: None,
        },
    }
}
