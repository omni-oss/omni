use std::{collections::HashMap, ffi::OsString, pin::Pin};

use bytes::{BufMut, Bytes, BytesMut};
use derive_new::new;
use futures::{
    AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _,
    future::try_join_all,
};
use maps::Map;
use omni_core::TaskExecutionNode;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;
use tokio::sync::mpsc;

use crate::{Child, ChildError};

#[derive(new)]
pub struct ChildProcess {
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

    #[new(default)]
    keep_stdin_open: bool,
}

#[auto_impl]
pub trait TaskExecutorWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait TaskExecutorReader: AsyncRead + Send {}

#[derive(Debug, Clone, PartialEq, Eq, new)]
pub struct ChildProcessResult {
    #[new(into)]
    pub node: TaskExecutionNode,
    #[new(into)]
    pub exit_code: u32,
    #[new(into)]
    pub elapsed: std::time::Duration,
    #[new(into)]
    pub logs: Option<Bytes>,
}

impl ChildProcessResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn exit_code(&self) -> u32 {
        self.exit_code
    }
}

impl ChildProcess {
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

    pub fn keep_stdin_open(&mut self, keep_stdin_open: bool) -> &mut Self {
        self.keep_stdin_open = keep_stdin_open;
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

    pub async fn exec(
        mut self,
    ) -> Result<ChildProcessResult, TaskExecutorError> {
        if self.task.task_command().is_empty() {
            return Ok(ChildProcessResult {
                node: self.task,
                exit_code: 0,
                elapsed: std::time::Duration::ZERO,
                logs: None,
            });
        }

        let start_time = std::time::Instant::now();

        let task = self.task;

        let command = if let Some(command) = self.expanded_command.as_ref() {
            command
        } else {
            task.task_command()
        };

        let parsed = shlex::split(command).ok_or_else(|| {
            TaskExecutorErrorInner::CantParseCommand(command.to_string())
        })?;

        let child = Child::spawn(
            parsed[0].clone(),
            parsed.iter().skip(1).cloned().collect::<Vec<_>>(),
            task.project_dir(),
            self.env_vars.unwrap_or_default(),
        )?;

        let stdout = child
            .take_output_reader()
            .ok_or(TaskExecutorErrorInner::CantTakeStdout)?;

        let stderr = child
            .take_error_reader()
            .ok_or(TaskExecutorErrorInner::CantTakeStderr)?;

        let mut input = child
            .take_input_writer()
            .ok_or(TaskExecutorErrorInner::CantTakeStdin)?;

        let mut tasks = vec![];

        let (logs_tx, mut logs_rx) = mpsc::channel::<Bytes>(256);

        let has_output_writer = self.output_writer.is_some();
        if has_output_writer || self.record_logs {
            {
                let logs_tx = logs_tx.clone();

                tasks.push(tokio::spawn(async move {
                    let mut buff = [0; 4096];
                    let mut reader = stdout.take(u64::MAX);

                    while let Ok(n) = reader.read(&mut buff).await {
                        if n == 0 {
                            trace::trace!("stdout is empty, breaking");
                            break;
                        }
                        logs_tx
                            .send(Bytes::copy_from_slice(&buff[..n]))
                            .await?;
                    }

                    trace::trace!("stdout task done");

                    Ok::<_, TaskExecutorError>(())
                }));
            }

            if let Some(stderr) = stderr {
                let logs_tx = logs_tx.clone();

                tasks.push(tokio::spawn(async move {
                    let mut buff = [0; 4096];
                    let mut reader = stderr.take(u64::MAX);

                    while let Ok(n) = reader.read(&mut buff).await {
                        if n == 0 {
                            trace::trace!("stderr is empty, breaking");
                            break;
                        }
                        logs_tx
                            .send(Bytes::copy_from_slice(&buff[..n]))
                            .await?;
                    }

                    trace::trace!("stderr task done");

                    Ok::<_, TaskExecutorError>(())
                }));
            }

            drop(logs_tx);
        }

        let mut writer = self.output_writer.take();
        let logs_output_task = tokio::spawn(async move {
            if !self.record_logs && self.output_writer.is_none() {
                return Ok::<_, TaskExecutorError>(None);
            }

            let mut logs_output = if self.record_logs {
                Some(BytesMut::new())
            } else {
                None
            };

            while let Some(buff) = logs_rx.recv().await {
                trace::debug!("received log chunk");
                if let Some(logs_output) = &mut logs_output {
                    trace::debug!("writing log chunk to logs output");
                    logs_output.put_slice(&buff[..]);
                }

                if let Some(writer) = writer.as_mut() {
                    trace::debug!("writing log chunk to output writer");
                    writer.write_all(&buff[..]).await?;
                }
            }
            trace::debug!("logs output task done");
            Ok::<_, TaskExecutorError>(logs_output.map(|b| b.freeze()))
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
        } else if !self.keep_stdin_open {
            std::mem::drop(input);
        }

        let all_tasks = try_join_all(tasks);

        let (vec_result, exit_status) = tokio::join!(all_tasks, child.wait());

        let _ = vec_result?;
        let logs = logs_output_task.await??;

        let exit_code = exit_status?;

        let elapsed = start_time.elapsed();

        Ok(ChildProcessResult {
            node: task,
            exit_code,
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
    CantRunCommand(#[from] ChildError),

    #[error("can't parse command: {0}")]
    CantParseCommand(String),

    // #[error("can't get env vars")]
    // CantGetEnvVars,
    #[error("cant't take stdin")]
    CantTakeStdin,

    #[error("cant't take stdout")]
    CantTakeStdout,

    #[error("cant't take stderr")]
    CantTakeStderr,

    // #[error("cant't take stderr")]
    // CantTakeStderr,
    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Mpsc(#[from] tokio::sync::mpsc::error::SendError<Bytes>),
}
