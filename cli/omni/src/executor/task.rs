use std::{
    collections::HashMap, ffi::OsString, process::ExitStatus, sync::Arc,
};

use derive_new::new;
use futures::AsyncReadExt as _;
use maps::Map;
use omni_core::TaskExecutionNode;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::impls::RealSys;
use tokio::sync::Mutex;

use crate::{
    context::{Context, ContextSys},
    executor::{CommandExecutor, CommandExecutorError},
};

#[derive(Debug, new)]
pub struct TaskExecutor<TContextSys: ContextSys = RealSys> {
    node: TaskExecutionNode,

    #[new(into)]
    context: Arc<Mutex<Context<TContextSys>>>,
}

impl TaskExecutor {
    pub async fn run(self) -> Result<ExitStatus, TaskExecutorError> {
        let task = self.node;
        let (command, vars_os) = {
            // Scope the lock to the duration of the task so that we don't hold the lock for the entire duration of the task
            //
            let mg = self.context.lock().await;

            let cached = mg
                .get_cached_env_vars(task.project_dir())
                .map_err(TaskExecutorErrorInner::CantGetEnvVars)?;

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

        let cmd_exec = CommandExecutor::from_comand_and_env(
            command,
            task.project_dir(),
            vars_os,
        )?;

        let stderr = cmd_exec
            .take_stderr()
            .ok_or(TaskExecutorErrorInner::CantTakeStderr)?;

        let stdout = cmd_exec
            .take_stdout()
            .ok_or(TaskExecutorErrorInner::CantTakeStdout)?;

        let input = cmd_exec
            .take_stdin()
            .ok_or(TaskExecutorErrorInner::CantTakeStdin)?;

        std::mem::drop(input);

        let stdout_task = tokio::task::spawn(async {
            futures::io::copy(
                &mut stdout.take(u64::MAX),
                &mut futures::io::AllowStdIo::new(std::io::stdout()),
            )
            .await?;

            Ok::<(), std::io::Error>(())
        });

        let stderr_task = tokio::task::spawn(async {
            futures::io::copy(
                &mut stderr.take(u64::MAX),
                &mut futures::io::AllowStdIo::new(std::io::stderr()),
            )
            .await?;

            Ok::<(), std::io::Error>(())
        });

        let exit_status = cmd_exec.run().await?;

        let (stdout_res, stderr_res) =
            tokio::try_join!(stdout_task, stderr_task)?;

        stdout_res?;
        stderr_res?;

        Ok(exit_status)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct TaskExecutorError {
    kind: TaskExecutorErrorKind,
    #[source]
    inner: TaskExecutorErrorInner,
}

impl TaskExecutorError {
    pub fn kind(&self) -> TaskExecutorErrorKind {
        self.kind
    }
}

impl<T: Into<TaskExecutorErrorInner>> From<T> for TaskExecutorError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

fn vars_os(vars: &Map<String, String>) -> HashMap<OsString, OsString> {
    vars.iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect::<HashMap<_, _>>()
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(TaskExecutorErrorKind), vis(pub), repr(u8))]
#[allow(clippy::enum_variant_names)]
enum TaskExecutorErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("can't run command: {0}")]
    CantRunCommand(#[from] CommandExecutorError),

    #[error("can't get env vars: {0}")]
    CantGetEnvVars(eyre::Report),

    #[error("cant't take stdin")]
    CantTakeStdin,

    #[error("cant't take stdout")]
    CantTakeStdout,

    #[error("cant't take stderr")]
    CantTakeStderr,

    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
}
