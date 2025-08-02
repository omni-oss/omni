use std::{
    cell::Cell,
    collections::HashMap,
    ffi::OsString,
    os::{fd::FromRawFd as _, unix::process::ExitStatusExt},
    path::PathBuf,
    process::ExitStatus,
};

use deno_task_shell::{ShellPipeReader, ShellPipeWriter};
use derive_new::new;
use futures::{AsyncRead, AsyncWrite};
use os_pipe::{PipeReader, PipeWriter};
use std::os::fd::IntoRawFd;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;
use tokio::fs::File;
use tokio_util::compat::{
    TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt,
};

use crate::utils;

#[allow(clippy::too_many_arguments)]
#[derive(new)]
pub struct CommandExecutor {
    #[new(into)]
    command: String,
    #[new(into)]
    cwd: PathBuf,
    #[new(into)]
    env: HashMap<OsString, OsString>,

    #[new(into)]
    stdin_reader: PipeReader,
    #[new(into)]
    stdout_writer: PipeWriter,
    #[new(into)]
    stderr_writer: PipeWriter,

    #[new(into)]
    stdin_writer_file: Cell<Option<File>>,
    #[new(into)]
    stdout_reader_file: Cell<Option<File>>,
    #[new(into)]
    stderr_reader_file: Cell<Option<File>>,
}

impl CommandExecutor {
    pub fn from_comand_and_env(
        command: impl Into<String>,
        cwd: impl Into<PathBuf>,
        env: impl Into<HashMap<OsString, OsString>>,
    ) -> Result<Self, CommandExecutorError> {
        let (stdin_reader, stdin_writer) = os_pipe::pipe()
            .map_err(CommandExecutorErrorInner::CantCreatePipe)?;
        let (stdout_reader, stdout_writer) = os_pipe::pipe()
            .map_err(CommandExecutorErrorInner::CantCreatePipe)?;
        let (stderr_reader, stderr_writer) = os_pipe::pipe()
            .map_err(CommandExecutorErrorInner::CantCreatePipe)?;

        let stdin_writer_fd = stdin_writer.into_raw_fd();
        let stdin_writer_file = unsafe { File::from_raw_fd(stdin_writer_fd) };
        let stdout_reader_fd = stdout_reader.into_raw_fd();
        let stdout_reader_file = unsafe { File::from_raw_fd(stdout_reader_fd) };
        let stderr_reader_fd = stderr_reader.into_raw_fd();
        let stderr_reader_file = unsafe { File::from_raw_fd(stderr_reader_fd) };

        Ok(Self::new(
            command.into(),
            cwd,
            env,
            stdin_reader,
            stdout_writer,
            stderr_writer,
            Some(stdin_writer_file),
            Some(stdout_reader_file),
            Some(stderr_reader_file),
        ))
    }
}

#[auto_impl]
pub trait CommandExecutorWriter: AsyncWrite + Sync + Send {}

#[auto_impl]
pub trait CommandExecutorReader: AsyncRead + Sync + Send {}

impl CommandExecutor {
    pub fn take_stdin(&self) -> Option<impl CommandExecutorWriter + use<>> {
        self.stdin_writer_file.take().map(|f| f.compat_write())
    }

    pub fn take_stdout(&self) -> Option<impl CommandExecutorReader + use<>> {
        self.stdout_reader_file.take().map(|f| f.compat())
    }

    pub fn take_stderr(&self) -> Option<impl CommandExecutorReader + use<>> {
        self.stderr_reader_file.take().map(|f| f.compat())
    }

    pub async fn run(self) -> Result<ExitStatus, CommandExecutorError> {
        let exit = utils::cmd::run_with_pipes(
            &self.command,
            &self.cwd,
            self.env,
            ShellPipeReader::from_raw(self.stdin_reader),
            ShellPipeWriter::OsPipe(self.stdout_writer),
            ShellPipeWriter::OsPipe(self.stderr_writer),
        )
        .await
        .map_err(CommandExecutorErrorInner::CantRunCommand)?;

        Ok(ExitStatus::from_raw(exit))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct CommandExecutorError {
    kind: CommandExecutorErrorKind,
    #[source]
    inner: CommandExecutorErrorInner,
}

impl CommandExecutorError {
    pub fn kind(&self) -> CommandExecutorErrorKind {
        self.kind
    }
}

impl<T: Into<CommandExecutorErrorInner>> From<T> for CommandExecutorError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(CommandExecutorErrorKind), vis(pub), repr(u8))]
enum CommandExecutorErrorInner {
    #[error("can't create pipe: {0}")]
    CantCreatePipe(std::io::Error),

    #[error("can't run command: {0}")]
    CantRunCommand(eyre::Report),
}
