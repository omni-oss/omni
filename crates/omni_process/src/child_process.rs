use std::{collections::HashMap, ffi::OsString, path::PathBuf, pin::Pin};

use bytes::{BufMut as _, Bytes, BytesMut};
use derive_new::new;
use futures::future::try_join_all;
use maps::Map;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;
use tokio::io::{
    AsyncBufReadExt as _, AsyncRead, AsyncReadExt as _, AsyncWrite,
    AsyncWriteExt as _, BufReader,
};
use trace::Level;

use crate::{Child, ChildError};

#[auto_impl]
pub trait ChildProcessWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait ChildProcessReader: AsyncRead + Send {}

#[derive(new, bon::Builder)]
pub struct ChildProcess {
    #[new(into)]
    program: String,

    #[new(into)]
    args: Vec<String>,

    #[new(into)]
    current_dir: PathBuf,

    #[new(default)]
    output_writer: Option<Pin<Box<dyn ChildProcessWriter>>>,

    #[new(default)]
    input_reader: Option<Pin<Box<dyn ChildProcessReader>>>,

    #[new(default)]
    env_vars: Option<HashMap<OsString, OsString>>,

    #[new(default)]
    record_logs: bool,

    #[new(default)]
    keep_stdin_open: bool,

    #[new(default)]
    empty_command_is_success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, new)]
pub struct ChildProcessResult {
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
        writer: impl ChildProcessWriter + 'static,
    ) -> &mut Self {
        self.output_writer = Some(Box::pin(writer));
        self
    }

    pub fn env_vars(&mut self, vars: &Map<String, String>) -> &mut Self {
        self.env_vars = Some(vars_os(vars));

        self
    }

    pub fn keep_stdin_open(&mut self, keep_stdin_open: bool) -> &mut Self {
        self.keep_stdin_open = keep_stdin_open;
        self
    }

    pub fn input_reader(
        &mut self,
        reader: impl ChildProcessReader + 'static,
    ) -> &mut Self {
        self.input_reader = Some(Box::pin(reader));

        self
    }

    pub fn record_logs(&mut self, record_logs: bool) -> &mut Self {
        self.record_logs = record_logs;
        self
    }

    pub fn empty_command_is_success(
        &mut self,
        empty_command_is_success: bool,
    ) -> &mut Self {
        self.empty_command_is_success = empty_command_is_success;
        self
    }

    #[cfg_attr(
        feature = "enable-tracing",
        tracing::instrument(level = Level::DEBUG, skip_all)
    )]
    pub async fn exec(
        mut self,
    ) -> Result<ChildProcessResult, ChildProcessError> {
        let program = std::mem::take(&mut self.program);
        let args = std::mem::take(&mut self.args);

        let program_is_empty = program.trim().is_empty();
        let args_is_empty = args.is_empty();

        if program_is_empty && args_is_empty {
            if self.empty_command_is_success {
                return Ok(ChildProcessResult {
                    exit_code: 0,
                    elapsed: std::time::Duration::default(),
                    logs: None,
                });
            } else {
                return Err(ChildProcessError::no_command());
            }
        } else if program_is_empty && !args_is_empty {
            return Err(ChildProcessError::empty_program_with_args(
                program, args,
            ));
        }

        let start_time = std::time::Instant::now();

        log::trace!("executing command: {:?}", args);

        let child = Child::spawn(
            program,
            args,
            std::mem::take(&mut self.current_dir),
            self.env_vars.take().unwrap_or_default(),
        )?;

        let stdout = child
            .take_output_reader()
            .ok_or(ChildProcessErrorInner::CantTakeStdout)?;

        let stderr = child
            .take_error_reader()
            .ok_or(ChildProcessErrorInner::CantTakeStderr)?;

        let mut input = child
            .take_input_writer()
            .ok_or(ChildProcessErrorInner::CantTakeStdin)?;

        let mut tasks = vec![];

        let mut writer = self.output_writer.take();
        let logs_output_task = tokio::spawn(async move {
            if !self.record_logs && writer.is_none() {
                log::trace!("no logs output, exit early");
                return Ok::<_, ChildProcessError>(None);
            }

            let mut logs_output = if self.record_logs {
                Some(BytesMut::new())
            } else {
                None
            };

            log::trace!("logs output task started");

            let mut stderr = stderr.map(BufReader::new);
            let mut stdout = BufReader::new(stdout);
            loop {
                let n;
                let line;
                if let Some(stderr_mut) = stderr.as_mut() {
                    let mut stdout_line = String::new();
                    let mut stderr_line = String::new();
                    tokio::select! {
                        res = stderr_mut.read_line(&mut stderr_line) => {
                            n = res?;
                            if n == 0 {
                                stderr = None;
                                log::trace!("stderr is empty, breaking");
                                continue;
                            }
                            line = stderr_line;
                        }
                        res = stdout.read_line(&mut stdout_line) => {
                            n = res?;
                            if n == 0 {
                                log::trace!("stdout is empty, breaking");
                                break;
                            }
                            line = stdout_line;
                        }
                    }
                } else {
                    let mut stdout_line = String::new();
                    n = stdout.read_line(&mut stdout_line).await?;
                    if n == 0 {
                        log::trace!("stdout is empty, breaking");
                        break;
                    }
                    line = stdout_line;
                }

                log::trace!("received log chunk to write: {}", n);

                if let Some(logs_output) = &mut logs_output {
                    log::trace!("writing log chunk to logs output");
                    logs_output.put_slice(line.as_bytes());
                }

                if let Some(writer) = writer.as_mut() {
                    log::trace!("writing log chunk to output writer");
                    writer.write_all(line.as_bytes()).await?;
                }
            }
            log::trace!("logs output task done");
            Ok::<_, ChildProcessError>(logs_output.map(|b| b.freeze()))
        });

        if let Some(input_reader) = self.input_reader {
            let stdin_task = {
                tokio::spawn(async move {
                    tokio::io::copy(
                        &mut input_reader.take(u64::MAX),
                        &mut input,
                    )
                    .await?;

                    Ok::<_, ChildProcessError>(())
                })
            };

            tasks.push(stdin_task);
        } else if !self.keep_stdin_open {
            trace::trace!("dropping_input");
            std::mem::drop(input);
        }

        let all_tasks = try_join_all(tasks);

        let (logs_output, vec_result, exit_status) =
            tokio::join!(logs_output_task, all_tasks, child.wait());

        let _ = vec_result?;
        let logs = logs_output??;

        let exit_code = exit_status?;

        let elapsed = start_time.elapsed();

        Ok(ChildProcessResult {
            exit_code,
            elapsed,
            logs,
        })
    }
}

fn vars_os(vars: &Map<String, String>) -> HashMap<OsString, OsString> {
    vars.iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect::<HashMap<_, _>>()
}

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct ChildProcessError(pub(crate) ChildProcessErrorInner);

impl ChildProcessError {
    pub fn custom<T: Into<eyre::Report>>(inner: T) -> Self {
        Self(ChildProcessErrorInner::Custom(inner.into()))
    }

    pub fn no_command() -> Self {
        Self(ChildProcessErrorInner::NoCommandProvided)
    }

    pub fn empty_program_with_args(
        program: impl Into<String>,
        args: impl Into<Vec<String>>,
    ) -> Self {
        Self(ChildProcessErrorInner::EmptyProgramWithArgs {
            program: program.into(),
            args: args.into(),
        })
    }
}

impl<T: Into<ChildProcessErrorInner>> From<T> for ChildProcessError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

impl ChildProcessError {
    #[allow(unused)]
    pub fn kind(&self) -> ChildProcessErrorKind {
        self.0.discriminant()
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ChildProcessErrorKind), vis(pub), repr(u8))]
#[allow(clippy::enum_variant_names)]
pub(crate) enum ChildProcessErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("can't run command: {0}")]
    CantRunCommand(#[from] ChildError),

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
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Mpsc(#[from] tokio::sync::mpsc::error::SendError<Bytes>),

    #[error("no command is provided")]
    NoCommandProvided,

    #[error("empty program with args: program=\'{program}\' args={args:?}")]
    EmptyProgramWithArgs { program: String, args: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit_zero_argv() -> (String, Vec<String>) {
        #[cfg(windows)]
        {
            (
                "cmd".to_string(),
                vec!["/C".to_string(), "exit".to_string(), "0".to_string()],
            )
        }
        #[cfg(not(windows))]
        {
            (
                "sh".to_string(),
                vec!["-c".to_string(), "sleep 1; exit 0".to_string()],
            )
        }
    }

    #[tokio::test]
    async fn spawns_known_argv_and_exits_zero() {
        let cwd = std::env::current_dir().unwrap();
        let (bin, args) = exit_zero_argv();
        let result = ChildProcess::new(bin, args, cwd)
            .exec()
            .await
            .expect("should spawn and exit");

        assert!(result.success());
        assert_eq!(result.exit_code(), 0);
    }

    #[tokio::test]
    async fn empty_program_with_empty_command_is_success() {
        let cwd = std::env::current_dir().unwrap();
        let mut child =
            ChildProcess::new("".to_string(), Vec::<String>::new(), cwd);
        child.empty_command_is_success(true);

        let result =
            child.exec().await.expect("empty command should be success");

        assert!(result.success());
    }

    #[tokio::test]
    async fn empty_program_without_flag_errors() {
        let cwd = std::env::current_dir().unwrap();
        let result =
            ChildProcess::new("".to_string(), Vec::<String>::new(), cwd)
                .exec()
                .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn empty_program_with_args_errors() {
        let cwd = std::env::current_dir().unwrap();
        let result =
            ChildProcess::new("".to_string(), vec!["arg".to_string()], cwd)
                .exec()
                .await;

        assert!(result.is_err());
    }
}
