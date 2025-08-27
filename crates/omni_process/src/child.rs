use std::{
    cell::Cell, collections::HashMap, ffi::OsString, io, path::PathBuf,
    pin::Pin, process::Stdio,
};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use derive_new::new;
use futures::{AsyncRead, AsyncWrite, io::AllowStdIo};
use portable_pty::{MasterPty, SlavePty, native_pty_system};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;
use tokio::task::yield_now;

use crate::utils::{get_pty_size, should_use_pty};

#[allow(clippy::too_many_arguments)]
#[derive(new)]
pub struct Child {
    inner: ChildInner,
    input: Cell<Option<Pin<Box<dyn ChildInputWriter>>>>,
    output: Cell<Option<Pin<Box<dyn ChildOutputReader>>>>,
    error: Cell<Option<Pin<Box<dyn ChildOutputReader>>>>,
    pid: Option<u32>,
}

impl Child {
    pub fn spawn(
        command: impl Into<String>,
        args: impl Into<Vec<String>>,
        cwd: impl Into<PathBuf>,
        env: impl Into<HashMap<OsString, OsString>>,
    ) -> Result<Self, ChildError> {
        let child =
            create_inner(command.into(), args.into(), cwd.into(), env.into())?;

        match child {
            Command::Pty(child) => Self::spawn_pty(child),
            Command::Normal(child) => Self::spawn_normal(child),
        }
    }

    fn spawn_pty(cmd: PtyCommand) -> Result<Self, ChildError> {
        let writer = cmd
            .master
            .take_writer()
            .map_err(|e| ChildErrorInner::CantTakeInputWriter(e.to_string()))?;

        let reader = cmd.master.try_clone_reader().map_err(|e| {
            ChildErrorInner::CantTakeOutputReader(e.to_string())
        })?;

        let child = cmd
            .slave
            .spawn_command(cmd.cmd)
            .map_err(|e| ChildErrorInner::CantSpawnCommand(e.to_string()))?;

        #[cfg(unix)]
        {
            use nix::sys::termios;
            use std::os::fd::AsFd;

            struct AsFdImp(i32);

            impl AsFd for AsFdImp {
                fn as_fd(&self) -> std::os::unix::prelude::BorrowedFd<'_> {
                    unsafe {
                        std::os::unix::prelude::BorrowedFd::borrow_raw(self.0)
                    }
                }
            }

            if let Some((file_desc, mut termios)) =
                cmd.master.as_raw_fd().and_then(|fd| {
                    Some(AsFdImp(fd)).zip(termios::tcgetattr(AsFdImp(fd)).ok())
                })
            {
                // We unset ECHOCTL to disable rendering of the closing of stdin
                // as ^D
                termios.local_flags &= !nix::sys::termios::LocalFlags::ECHOCTL;
                if let Err(e) = nix::sys::termios::tcsetattr(
                    file_desc,
                    nix::sys::termios::SetArg::TCSANOW,
                    &termios,
                ) {
                    trace::debug!("failed to set termios: {e}");
                }
            }
        }
        let pid = child.process_id();

        Ok(Self::new(
            ChildInner::Pty(child),
            Cell::new(Some(Box::pin(AllowStdIo::new(writer)))),
            Cell::new(Some(Box::pin(AllowStdIo::new(reader)))),
            Cell::new(None),
            pid,
        ))
    }

    fn spawn_normal(mut cmd: StdCommand) -> Result<Self, ChildError> {
        let builder = cmd
            .cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Create a process group for the child on unix like systems
        #[cfg(unix)]
        {
            use nix::unistd::setsid;
            unsafe {
                builder.pre_exec(|| {
                    setsid()?;
                    Ok(())
                });
            }
        }

        let mut child = builder
            .spawn()
            .map_err(|e| ChildErrorInner::CantSpawnCommand(e.to_string()))?;

        let stdout = child.stdout.take().ok_or_else(|| {
            ChildErrorInner::CantTakeOutputReader("no stdout".to_string())
        })?;

        let stderr = child.stderr.take().ok_or_else(|| {
            ChildErrorInner::CantTakeErrorReader("no stderr".to_string())
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ChildErrorInner::CantTakeInputWriter("no stdin".to_string())
        })?;

        let pid = child.id();

        Ok(Self::new(
            ChildInner::Normal(child),
            Cell::new(Some(Box::pin(stdin.compat_write()))),
            Cell::new(Some(Box::pin(stdout.compat()))),
            Cell::new(Some(Box::pin(stderr.compat()))),
            pid,
        ))
    }
}

#[auto_impl]
pub trait ChildInputWriter: AsyncWrite + Send {}

#[auto_impl]
pub trait ChildOutputReader: AsyncRead + Send {}

impl Child {
    pub fn take_input_writer(&self) -> Option<impl ChildInputWriter + use<>> {
        self.input.take()
    }

    pub fn take_output_reader(&self) -> Option<impl ChildOutputReader + use<>> {
        self.output.take()
    }

    pub fn take_error_reader(&self) -> Option<impl ChildOutputReader + use<>> {
        self.error.take()
    }
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    #[tracing::instrument(skip_all)]
    pub async fn wait(self) -> Result<u32, ChildError> {
        match self.inner {
            ChildInner::Pty(mut child) => {
                yield_now().await;

                let status = child.wait().inspect_err(|e| {
                    trace::error!("wait error: {e}");
                })?;

                Ok(status.exit_code())
            }
            ChildInner::Normal(mut child) => {
                let status = child.wait().await.inspect_err(|e| {
                    trace::error!("wait error: {e}");
                })?;

                Ok(status.code().unwrap_or(0) as u32)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ChildError {
    kind: ChildErrorKind,
    #[source]
    inner: ChildErrorInner,
}

impl ChildError {
    pub fn kind(&self) -> ChildErrorKind {
        self.kind
    }
}

impl<T: Into<ChildErrorInner>> From<T> for ChildError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ChildErrorKind), vis(pub), repr(u8))]
#[allow(clippy::enum_variant_names)]
enum ChildErrorInner {
    #[error("can't spawn command: {0}")]
    CantSpawnCommand(String),

    #[error("can't open pty")]
    CantOpenPty,

    #[error("can't take output reader: {0}")]
    CantTakeOutputReader(String),

    #[error("can't take error reader: {0}")]
    CantTakeErrorReader(String),

    #[error("can't take input writer: {0}")]
    CantTakeInputWriter(String),

    #[error(transparent)]
    Io(#[from] io::Error),
}

fn create_inner(
    command: String,
    args: Vec<String>,
    cwd: PathBuf,
    env: HashMap<OsString, OsString>,
) -> Result<Command, ChildErrorInner> {
    if should_use_pty() {
        let pty_sys = native_pty_system();
        let pty = pty_sys
            .openpty(get_pty_size())
            .map_err(|_e| ChildErrorInner::CantOpenPty)?;

        let mut pty_cmd = portable_pty::CommandBuilder::new(&command);
        pty_cmd.args(args);
        pty_cmd.cwd(cwd);
        for (k, v) in env.into_iter() {
            pty_cmd.env(k, v);
        }

        Ok(Command::Pty(PtyCommand {
            cmd: pty_cmd,
            slave: pty.slave,
            master: pty.master,
        }))
    } else {
        let mut std_cmd = tokio::process::Command::new(command);

        std_cmd.args(args);
        std_cmd.current_dir(cwd);
        std_cmd.envs(env);

        Ok(Command::Normal(StdCommand { cmd: std_cmd }))
    }
}

struct PtyCommand {
    cmd: portable_pty::CommandBuilder,
    slave: Box<dyn SlavePty + Send>,
    master: Box<dyn MasterPty + Send>,
}

struct StdCommand {
    cmd: tokio::process::Command,
}

enum Command {
    Pty(PtyCommand),
    Normal(StdCommand),
}

enum ChildInner {
    Pty(Box<dyn portable_pty::Child + Send + Sync>),
    Normal(tokio::process::Child),
}
