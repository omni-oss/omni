use std::collections::HashMap;

use crate::{
    error::with_notices,
    model::{
        ExecCommandParams, ExecCommandResult, TaskExecutionSummary,
        TaskRunParams, TaskRunResult,
    },
    server::OmniMcpServer,
    subscriber::McpSubscriber,
};
use omni_api::{ExecRequest, TaskRunFilters, TaskRunRequest};
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::{OnFailure, TaskExecutionResult, TaskExecutorSys};

impl<TSys> OmniMcpServer<TSys>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub(crate) async fn tool_task_run(
        &self,
        params: TaskRunParams,
    ) -> eyre::Result<TaskRunResult> {
        if let Some(retry) = &params.retry {
            retry.check_layer()?;
        }
        let include_logs = params.include_logs.unwrap_or_default();
        let filters = TaskRunFilters {
            meta: params.meta,
            meta_on_error: params.meta_on_error,
            meta_error_samples: params.meta_error_samples,
            project: params.project,
            dir: params.dir,
            dry_run: params.dry_run,
            args: params.args,
            retry: params.retry,
            ..Default::default()
        };
        let req = TaskRunRequest {
            tasks: params.tasks,
            ignore_dependencies: params.ignore_dependencies,
            force: params.force,
            filters,
            with_dependents: false,
            on_failure: OnFailure::SkipDependents,
            no_cache: false,
            output_logs: Some(include_logs),
            output_cached_logs: None,
        };
        let subscriber = McpSubscriber::new(include_logs);
        let response = match self
            .make_api_with_subscriber(&subscriber)?
            .task_run(req)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(with_notices(e, subscriber.take_notices()));
            }
        };
        let ok = response.is_success();
        let mut logs = subscriber.take_logs();
        let commands = subscriber.take_commands();
        let results = response
            .results
            .into_iter()
            .map(|r| summarize_result(r, &mut logs, &commands))
            .collect();
        Ok(TaskRunResult {
            ok,
            results,
            notices: subscriber.take_notices(),
        })
    }

    pub(crate) async fn tool_exec_command(
        &self,
        params: ExecCommandParams,
    ) -> eyre::Result<ExecCommandResult> {
        if let Some(retry) = &params.retry {
            retry.check_layer()?;
        }
        let include_logs = params.include_logs.unwrap_or_default();
        let filters = TaskRunFilters {
            project: params.project,
            dir: params.dir,
            dry_run: params.dry_run,
            retry: params.retry,
            ..Default::default()
        };
        let req = ExecRequest {
            cmd: params.cmd,
            filters,
            output_logs: Some(include_logs),
            output_cached_logs: Some(include_logs),
        };
        let subscriber = McpSubscriber::new(include_logs);
        let response =
            match self.make_api_with_subscriber(&subscriber)?.exec(req).await {
                Ok(r) => r,
                Err(e) => {
                    return Err(with_notices(e, subscriber.take_notices()));
                }
            };
        let ok = response.is_success();
        let mut logs = subscriber.take_logs();
        let commands = subscriber.take_commands();
        let results = response
            .results
            .into_iter()
            .map(|r| summarize_result(r, &mut logs, &commands))
            .collect();
        Ok(ExecCommandResult {
            ok,
            results,
            notices: subscriber.take_notices(),
        })
    }
}

fn summarize_result(
    result: TaskExecutionResult,
    logs: &mut HashMap<String, String>,
    commands: &HashMap<String, String>,
) -> TaskExecutionSummary {
    match result {
        TaskExecutionResult::Completed {
            task,
            exit_code,
            elapsed,
            ..
        } => {
            let task_logs = logs.remove(task.full_task_name());
            TaskExecutionSummary {
                project: task.project_name().to_string(),
                task: task.task_name().to_string(),
                status: "completed".to_string(),
                duration_ms: Some(elapsed.as_millis() as u64),
                exit_code: Some(exit_code),
                logs: task_logs,
                command: commands.get(task.full_task_name()).cloned(),
            }
        }
        TaskExecutionResult::Errored { task, .. } => {
            let task_logs = logs.remove(task.full_task_name());
            TaskExecutionSummary {
                project: task.project_name().to_string(),
                task: task.task_name().to_string(),
                status: "errored".to_string(),
                duration_ms: None,
                exit_code: None,
                logs: task_logs,
                command: commands.get(task.full_task_name()).cloned(),
            }
        }
        TaskExecutionResult::Skipped {
            task, skip_reason, ..
        } => TaskExecutionSummary {
            project: task.project_name().to_string(),
            task: task.task_name().to_string(),
            status: format!("skipped:{skip_reason:?}"),
            duration_ms: None,
            exit_code: None,
            logs: None,
            command: None,
        },
    }
}
