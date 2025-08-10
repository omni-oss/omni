use std::{
    collections::HashMap, ffi::OsString, pin::Pin, process::ExitStatus,
    sync::Arc,
};

use derive_new::new;
use futures::{AsyncRead, AsyncReadExt as _, AsyncWrite};
use maps::Map;
use omni_core::TaskExecutionNode;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::{auto_impl, impls::RealSys};

use crate::{
    context::{Context, ContextSys},
    executor::{CommandExecutor, CommandExecutorError},
};

#[derive(new)]
pub struct TaskExecutor<TContextSys: ContextSys = RealSys> {
    #[new(into)]
    node: TaskExecutionNode,

    #[new(into)]
    context: Arc<Context<TContextSys>>,

    #[new(default)]
    output_writer: Option<Pin<Box<dyn TaskExecutorWriter>>>,

    #[new(default)]
    input_reader: Option<Pin<Box<dyn TaskExecutorReader>>>,
}

#[auto_impl]
pub trait TaskExecutorWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait TaskExecutorReader: AsyncRead + Send {}

impl TaskExecutor {
    pub fn set_output_writer(
        &mut self,
        writer: impl TaskExecutorWriter + 'static,
    ) {
        self.output_writer = Some(Box::pin(writer));
    }

    pub fn set_input_reader(
        &mut self,
        reader: impl TaskExecutorReader + 'static,
    ) {
        self.input_reader = Some(Box::pin(reader));
    }

    pub async fn run(self) -> Result<ExitStatus, TaskExecutorError> {
        let task = self.node;
        let (command, vars_os) = {
            // Scope the lock to the duration of the task so that we don't hold the lock for the entire duration of the task
            //
            let mg = self.context;

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

        let cmd_exec = CommandExecutor::from_command_and_env(
            command,
            task.project_dir(),
            vars_os,
        )?;

        let stdout = cmd_exec
            .take_reader()
            .ok_or(TaskExecutorErrorInner::CantTakeStdout)?;

        let mut input = cmd_exec
            .take_writer()
            .ok_or(TaskExecutorErrorInner::CantTakeStdin)?;

        let mut tasks = vec![];

        if let Some(mut output_writer) = self.output_writer {
            let stdout_task = {
                tokio::spawn(async move {
                    futures::io::copy(
                        &mut stdout.take(u64::MAX),
                        &mut output_writer,
                    )
                    .await
                })
            };

            tasks.push(stdout_task);
        }
        if let Some(input_reader) = self.input_reader {
            let stdin_task = {
                tokio::spawn(async move {
                    futures::io::copy(
                        &mut input_reader.take(u64::MAX),
                        &mut input,
                    )
                    .await
                })
            };

            tasks.push(stdin_task);
        } else {
            std::mem::drop(input);
        }

        let all_tasks = futures::future::join_all(tasks);

        let (vec_result, exit_status) = tokio::join!(all_tasks, cmd_exec.run());

        for result in vec_result {
            result??;
        }

        let exit_status = exit_status?;

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
