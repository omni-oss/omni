use std::{
    collections::HashMap,
    ffi::OsString,
    os::fd::{FromRawFd, IntoRawFd},
    sync::Arc,
};

use deno_task_shell::{ShellPipeReader, ShellPipeWriter};
use maps::Map;
use omni_core::TaskExecutionNode;
use tokio::{io::AsyncReadExt as _, sync::Mutex};

use crate::{
    context::Context,
    utils::{self},
};

async fn execute_task_impl(
    task: TaskExecutionNode,
    ctx: Arc<Mutex<Context>>,
) -> Result<TaskExecutionNode, (TaskExecutionNode, eyre::Report)> {
    let (command, vars_os) = {
        // Scope the lock to the duration of the task so that we don't hold the lock for the entire duration of the task
        //
        let mg = ctx.lock().await;

        let cached = mg
            .get_cached_env_vars(task.project_dir())
            .map_err(|e| (task.clone(), e))?;

        if let Some(task_vars) = mg.get_task_env_vars(&task) {
            let total = cached.len() + task_vars.len();
            let mut vars = maps::map!(cap: total);

            vars.extend(cached.clone());
            let mut task_vars = task_vars.clone();
            env::expand_into(&mut task_vars, &vars);
            vars.extend(task_vars);

            (::env::expand(task.task_command(), &vars), vars_os(&vars))
        } else {
            (::env::expand(task.task_command(), cached), vars_os(cached))
        }
    };

    trace::debug!(
        "Running command: '{:?}' in dir: {:?}",
        command,
        task.project_dir()
    );

    let map_err = |e: std::io::Error| -> (TaskExecutionNode, eyre::Report) {
        (task.clone(), eyre::eyre!(e))
    };

    let (input_reader, input_writer) = os_pipe::pipe().map_err(map_err)?;
    let (output_reader, output_writer) = os_pipe::pipe().map_err(map_err)?;
    let (error_reader, error_writer) = os_pipe::pipe().map_err(map_err)?;

    // Drop the writers so that the readers can be read until the end
    std::mem::drop(input_writer);

    let stdout_task = {
        use tokio::fs::File;
        let task = task.clone();
        let file = unsafe { File::from_raw_fd(output_reader.into_raw_fd()) };

        tokio::task::spawn(async {
            tokio::io::copy(&mut file.take(u64::MAX), &mut tokio::io::stdout())
                .await
                .map_err(|e| (task.clone(), eyre::eyre!(e)))?;

            Ok::<TaskExecutionNode, (TaskExecutionNode, eyre::Report)>(task)
        })
    };

    let stderr_task = {
        use tokio::fs::File;
        let task = task.clone();
        let file = unsafe { File::from_raw_fd(error_reader.into_raw_fd()) };

        tokio::task::spawn(async {
            tokio::io::copy(&mut file.take(u64::MAX), &mut tokio::io::stderr())
                .await
                .map_err(|e| (task.clone(), eyre::eyre!(e)))?;

            Ok::<TaskExecutionNode, (TaskExecutionNode, eyre::Report)>(task)
        })
    };

    let exit = utils::cmd::run_with_pipes(
        &command,
        task.project_dir(),
        vars_os,
        ShellPipeReader::from_raw(input_reader),
        ShellPipeWriter::OsPipe(output_writer),
        ShellPipeWriter::OsPipe(error_writer),
    )
    .await
    .map_err(|e| (task.clone(), e))?;

    let (stdout_res, stderr_res) =
        tokio::try_join!(stdout_task, stderr_task)
            .map_err(|e| (task.clone(), eyre::eyre!(e)))?;

    stdout_res?;
    stderr_res?;

    if exit != 0 {
        let error = eyre::eyre!("exited with code {}", exit);
        return Err((task, error));
    }
    Ok(task)
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
