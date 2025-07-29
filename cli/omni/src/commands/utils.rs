use std::{collections::HashMap, ffi::OsString, path::Path, sync::Arc};

use omni_core::TaskExecutionNode;
use tokio::sync::Mutex;

use crate::{context::Context, utils};

type Envs = (HashMap<String, String>, HashMap<OsString, OsString>);

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

        if !hm.contains_key(project_dir_str) {
            let mg = ctx.lock().await;
            let vars = mg
                .get_cached_env_vars(Path::new(task.project_dir()))
                .map_err(|e| (task.clone(), e))?;

            let vars_os = vars_os(vars);

            hm.insert(project_dir_str.to_string(), (vars.clone(), vars_os));
        }

        let envs = hm
            .get(project_dir_str)
            .ok_or_else(|| eyre::eyre!("Should be in map at this point"))
            .map_err(|e| (task.clone(), e))?;

        (::env::expand(task.task_command(), &envs.0), envs.1.clone())
    };

    trace::debug!(
        "Running command: '{:?}' with env: {:#?}, in dir: {:?}",
        command,
        vars_os,
        task.project_dir()
    );

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

fn vars_os(vars: &HashMap<String, String>) -> HashMap<OsString, OsString> {
    vars.iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect::<HashMap<_, _>>()
}
