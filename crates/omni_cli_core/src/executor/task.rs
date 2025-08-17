use std::{collections::HashMap, ffi::OsString, pin::Pin, process::ExitStatus};

use bytes::{BufMut, Bytes, BytesMut};
use derive_new::new;
use futures::{
    AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _,
    future::try_join_all,
};
use maps::Map;
use omni_cache::impls::LocalTaskExecutionCacheStoreError;
use omni_core::TaskExecutionNode;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;

use crate::executor::{CommandExecutor, CommandExecutorError};

#[derive(new)]
pub struct TaskExecutor {
    #[new(into)]
    task: TaskExecutionNode,

    #[new(default)]
    expanded_command: Option<String>,

    #[new(default)]
    output_writer: Option<Pin<Box<dyn TaskExecutorWriter>>>,

    #[new(default)]
    input_reader: Option<Pin<Box<dyn TaskExecutorReader>>>,

    #[new(default)]
    env_vars: Option<HashMap<OsString, OsString>>,

    #[new(default)]
    record_logs: bool,
}

#[auto_impl]
pub trait TaskExecutorWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait TaskExecutorReader: AsyncRead + Send {}

#[derive(Debug, Clone, PartialEq, Eq, new)]
pub struct ExecutionResult {
    #[new(into)]
    pub node: TaskExecutionNode,
    #[new(into)]
    pub exit_code: ExitStatus,
    #[new(into)]
    pub elapsed: std::time::Duration,
    #[new(into)]
    pub logs: Option<Bytes>,
}

impl ExecutionResult {
    pub fn success(&self) -> bool {
        self.exit_code.success()
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code.code().unwrap_or(0)
    }
}

impl TaskExecutor {
    pub fn output_writer(
        &mut self,
        writer: impl TaskExecutorWriter + 'static,
    ) -> &mut Self {
        self.output_writer = Some(Box::pin(writer));
        self
    }

    pub fn env_vars(&mut self, vars: &Map<String, String>) -> &mut Self {
        self.expanded_command =
            Some(::env::expand(self.task.task_command(), vars));
        self.env_vars = Some(vars_os(vars));

        self
    }

    pub fn input_reader(
        &mut self,
        reader: impl TaskExecutorReader + 'static,
    ) -> &mut Self {
        self.input_reader = Some(Box::pin(reader));

        self
    }

    pub fn record_logs(&mut self, record_logs: bool) -> &mut Self {
        self.record_logs = record_logs;
        self
    }

    pub async fn exec(self) -> Result<ExecutionResult, TaskExecutorError> {
        let start_time = std::time::Instant::now();

        let task = self.task;

        let command = if let Some(command) = self.expanded_command.as_ref() {
            command
        } else {
            task.task_command()
        };

        let cmd_exec = CommandExecutor::from_command_and_env(
            command,
            task.project_dir(),
            self.env_vars.unwrap_or_default(),
        )?;

        let mut stdout = cmd_exec
            .take_reader()
            .ok_or(TaskExecutorErrorInner::CantTakeStdout)?;

        let mut input = cmd_exec
            .take_writer()
            .ok_or(TaskExecutorErrorInner::CantTakeStdin)?;

        let mut tasks = vec![];

        let stdout_task = tokio::spawn(async move {
            if !self.record_logs && self.output_writer.is_none() {
                return Ok::<_, TaskExecutorError>(None);
            }

            let mut bytes = if self.record_logs {
                Some(BytesMut::new())
            } else {
                None
            };

            let mut buff = [0; 4096];
            let mut writer = self.output_writer;

            while let Ok(n) = stdout.read(&mut buff).await
                && n > 0
            {
                if let Some(bytes) = &mut bytes {
                    bytes.put_slice(&buff[..n]);
                }

                if let Some(writer) = writer.as_mut() {
                    writer.write_all(&buff[..n]).await?;
                }
            }

            Ok::<_, TaskExecutorError>(bytes.map(|b| b.freeze()))
        });

        if let Some(input_reader) = self.input_reader {
            let stdin_task = {
                tokio::spawn(async move {
                    futures::io::copy(
                        &mut input_reader.take(u64::MAX),
                        &mut input,
                    )
                    .await?;

                    Ok::<_, TaskExecutorError>(())
                })
            };

            tasks.push(stdin_task);
        } else {
            std::mem::drop(input);
        }

        let all_tasks = try_join_all(tasks);

        let (vec_result, stdout, exit_status) =
            tokio::join!(all_tasks, stdout_task, cmd_exec.run());

        let _ = vec_result?;
        let logs = stdout??;

        let exit_status = exit_status?;

        let elapsed = start_time.elapsed();

        Ok(ExecutionResult {
            node: task,
            exit_code: exit_status,
            elapsed,
            logs,
        })
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

    // #[error("can't get env vars")]
    // CantGetEnvVars,
    #[error("cant't take stdin")]
    CantTakeStdin,

    #[error("cant't take stdout")]
    CantTakeStdout,

    // #[error("cant't take stderr")]
    // CantTakeStderr,
    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),

    #[error(transparent)]
    LocalTaskExecutionCacheStore(#[from] LocalTaskExecutionCacheStoreError),
}
