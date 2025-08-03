use std::{collections::HashMap, ffi::OsString, sync::Arc};

use maps::Map;
use omni_core::TaskExecutionNode;
use tokio::sync::Mutex;

use crate::{context::Context, executor::TaskExecutor};

async fn execute_task_impl(
    task: TaskExecutionNode,
    ctx: Arc<Mutex<Context>>,
) -> Result<TaskExecutionNode, (TaskExecutionNode, eyre::Report)> {
    let result = {
        let task = task.clone();
        TaskExecutor::new(task, ctx).run().await
    };

    match result {
        Ok(status) => {
            if status.success() {
                Ok(task)
            } else {
                let error = eyre::eyre!("exited with code {}", status);
                Err((task, error))
            }
        }
        Err(e) => {
            let error = eyre::eyre!(e);
            Err((task, error))
        }
    }
}

pub async fn execute_task(
    task: TaskExecutionNode,
    ctx: Arc<Mutex<Context>>,
) -> Result<TaskExecutionNode, (TaskExecutionNode, eyre::Report)> {
    let full_task_name =
        format!("{}#{}", task.project_name(), task.task_name());
    trace::info!("Running task '{}'", full_task_name);

    let result = execute_task_impl(task, ctx).await;

    match result {
        Ok(t) => Ok(t),
        Err((task, error)) => {
            trace::error!("Task '{full_task_name}' failed with error: {error}");
            Err((task, error))
        }
    }
}

fn vars_os(vars: &Map<String, String>) -> HashMap<OsString, OsString> {
    vars.iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect::<HashMap<_, _>>()
}
