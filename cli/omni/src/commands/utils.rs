use std::{collections::HashMap, ffi::OsString, sync::Arc};

use omni_core::TaskExecutionNode;
use tokio::sync::Mutex;

use crate::{context::Context, utils};

type Envs = (HashMap<String, String>, HashMap<OsString, OsString>);

macro_rules! record {
    [$($key:expr => $value:expr),*$(,)?] => {{
        let mut hm = HashMap::<String, String>::new();
        $(
            hm.insert($key.to_string(), $value.to_string());
        )*

        hm
    }};
}

async fn execute_task_impl(
    task: TaskExecutionNode,
    ctx: Arc<Mutex<Context>>,
    dir_envs: Arc<Mutex<HashMap<String, Envs>>>,
) -> Result<TaskExecutionNode, (TaskExecutionNode, eyre::Report)> {
    let project_dir_str = task
        .project_dir()
        .to_str()
        .expect("Can't convert project dir to str");
    let (command, vars_os) = {
        // Scope the lock to the duration of the task so that we don't hold the lock for the entire duration of the task
        let mut hm = dir_envs.lock().await;

        let extras = record![
            "PROJECT_NAME" => task.project_name(),
            "TASK_NAME" => task.task_name(),
            "PROJECT_DIR" => task.project_dir().to_string_lossy()
        ];

        if !hm.contains_key(project_dir_str) {
            let (vars, vars_os) = ctx
                .lock()
                .await
                .get_env_vars_at_start_dir(
                    task.project_dir()
                        .to_str()
                        .expect("Can't convert project dir to str"),
                    Some(&extras),
                )
                .map_err(|e| (task.clone(), e.into()))?;

            hm.insert(project_dir_str.to_string(), (vars, vars_os));
        }

        let envs = hm
            .get(project_dir_str)
            .ok_or_else(|| eyre::eyre!("Should be in map at this point"))
            .map_err(|e| (task.clone(), e))?;

        (::env::expand(task.task_command(), &envs.0), envs.1.clone())
    };

    let exit = utils::cmd::run(&command, task.project_dir(), vars_os)
        .await
        .map_err(|e| (task.clone(), e))?;
    if exit != 0 {
        let error = eyre::eyre!("exited with code {}", exit);
        return Err((task, error));
    }
    Ok(task)
}

pub async fn execute_task(
    task: TaskExecutionNode,
    ctx: Arc<Mutex<Context>>,
    dir_envs: Arc<Mutex<HashMap<String, Envs>>>,
) -> Result<TaskExecutionNode, (TaskExecutionNode, eyre::Report)> {
    let full_task_name =
        format!("{}#{}", task.project_name(), task.task_name());
    trace::info!("Running task '{}'", full_task_name);

    let result = execute_task_impl(task, ctx, dir_envs).await;

    match result {
        Ok(t) => Ok(t),
        Err((task, error)) => {
            trace::error!("Task '{full_task_name}' failed with error: {error}");
            Err((task, error))
        }
    }
}
