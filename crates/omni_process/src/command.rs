use std::{
    cell::Cell, collections::HashMap, ffi::OsString, path::PathBuf, pin::Pin,
};

use derive_new::new;
use futures::{AsyncRead, AsyncWrite};
use portable_pty::{CommandBuilder, PtySize, SlavePty, native_pty_system};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;
use terminal_size::{Height, Width};
use tokio::task::yield_now;

fn get_pty_size() -> PtySize {
    let terminal_size =
        terminal_size::terminal_size().unwrap_or((Width(80), Height(24)));
    PtySize {
        cols: terminal_size.0.0,
        rows: terminal_size.1.0,
        pixel_height: 0,
        pixel_width: 0,
    }
}

#[allow(clippy::too_many_arguments)]
#[derive(new)]
pub struct CommandExecutor {
    #[new(into)]
    command: String,

    #[new(into)]
    args: Vec<String>,

    #[new(into)]
    cwd: PathBuf,
    #[new(into)]
    env: HashMap<OsString, OsString>,

    #[new(into)]
    writer: Cell<Option<Pin<Box<dyn CommandExecutorWriter>>>>,
    #[new(into)]
    reader: Cell<Option<Pin<Box<dyn CommandExecutorReader>>>>,

    #[new(into)]
    slave: Box<dyn SlavePty + Send>,
}

impl CommandExecutor {
    pub fn from_command_and_env(
        command: impl Into<String>,
        args: impl Into<Vec<String>>,
        cwd: impl Into<PathBuf>,
        env: impl Into<HashMap<OsString, OsString>>,
    ) -> Result<Self, CommandExecutorError> {
        let pty_sys = native_pty_system();
        let pty = pty_sys
            .openpty(get_pty_size())
            .map_err(|_e| CommandExecutorErrorInner::CantOpenPty)?;

        let reader = pty.master.try_clone_reader().map_err(|e| {
            CommandExecutorErrorInner::CantCloneReader(e.to_string())
        })?;

        let reader: Pin<Box<dyn CommandExecutorReader>> =
            Box::pin(futures::io::AllowStdIo::new(reader));

        let writer = pty.master.take_writer().map_err(|e| {
            CommandExecutorErrorInner::CantTakeWriter(e.to_string())
        })?;

        let writer: Pin<Box<dyn CommandExecutorWriter>> =
            Box::pin(futures::io::AllowStdIo::new(writer));

        Ok(Self::new(
            command.into(),
            args,
            cwd,
            env,
            Some(writer),
            Some(reader),
            pty.slave,
        ))
    }
}

#[auto_impl]
pub trait CommandExecutorWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait CommandExecutorReader: AsyncRead + Send {}

impl CommandExecutor {
    pub fn take_writer(&self) -> Option<impl CommandExecutorWriter + use<>> {
        self.writer.take()
    }

    pub fn take_reader(&self) -> Option<impl CommandExecutorReader + use<>> {
        self.reader.take()
    }

    pub async fn run(self) -> Result<u32, CommandExecutorError> {
        let mut cmd = CommandBuilder::new(self.command);

        cmd.args(self.args);

        cmd.cwd(&self.cwd);

        for (k, v) in self.env.iter() {
            cmd.env(k, v);
        }

        let mut child = self.slave.spawn_command(cmd).map_err(|e| {
            CommandExecutorErrorInner::CantSpawnCommand(e.to_string())
        })?;

        yield_now().await;
        let status = child
            .wait()
            .expect("child process should not be terminated");

        Ok(status.exit_code())
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
#[allow(clippy::enum_variant_names)]
enum CommandExecutorErrorInner {
    // #[error("can't create pipe: {0}")]
    // CantCreatePipe(std::io::Error),
    #[error("can't spawn command: {0}")]
    CantSpawnCommand(String),

    #[error("can't open pty")]
    CantOpenPty,

    #[error("can't clone reader: {0}")]
    CantCloneReader(String),

    #[error("can't take writer: {0}")]
    CantTakeWriter(String),
    // #[error("can't parse command: {0}")]
    // CantParseCommand(eyre::Report),
}
