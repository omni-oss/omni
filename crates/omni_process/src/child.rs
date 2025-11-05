use std::{
    cell::Cell, collections::HashMap, ffi::OsString, io, path::PathBuf,
    pin::Pin, process::Stdio,
};

use derive_new::new;
use portable_pty::{MasterPty, SlavePty, native_pty_system};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::auto_impl;
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _},
    task::JoinHandle,
};
use tracing::{dispatcher, field};

use crate::utils::{get_pty_size, should_use_pty};

pub type DynChildOutputReader = Box<dyn ChildOutputReader>;
pub type DynChildInputWriter = Box<dyn ChildInputWriter>;

#[allow(clippy::too_many_arguments)]
#[derive(new)]
pub struct Child {
    inner: ChildInner,
    input: Cell<Option<Pin<DynChildInputWriter>>>,
    output: Cell<Option<Pin<DynChildOutputReader>>>,
    error: Cell<Option<Option<Pin<DynChildOutputReader>>>>,
    pid: Option<u32>,
}

impl Child {
    #[cfg_attr(
        feature="enable-tracing",
        tracing::instrument(skip_all, fields(command = field::Empty, args = field::Empty))
    )]
    pub fn spawn(
        command: impl Into<String>,
        args: impl Into<Vec<String>>,
        cwd: impl Into<PathBuf>,
        env: impl Into<HashMap<OsString, OsString>>,
    ) -> Result<Self, ChildError> {
        let command = command.into();
        let args = args.into();

        if cfg!(feature = "enable-tracing") {
            let span = tracing::Span::current();

            span.record("command", &command);
            span.record("args", format_args!("{args:?}"));
        }

        let child = create_inner(command, args, cwd.into(), env.into())?;

        match child {
            Command::Pty(child) => Self::spawn_pty(child),
            Command::Normal(child) => Self::spawn_normal(child),
        }
    }

    fn spawn_pty(cmd: PtyCommand) -> Result<Self, ChildError> {
        trace::trace!("spawning pty");

        let mut writer = cmd
            .master
            .take_writer()
            .map_err(|e| ChildErrorInner::CantTakeInputWriter(e.to_string()))?;

        let mut reader = cmd.master.try_clone_reader().map_err(|e| {
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
                } else {
                    trace::trace!("termios set");
                }
            }
        }
        let pid = child.process_id();

        let (mut out_writer, out_reader) = tokio::io::duplex(1024);
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(1024);
        let (in_writer, mut in_reader) = tokio::io::duplex(1024);

        let dispatch = dispatcher::get_default(|d| d.clone());

        let reader_task = tokio::task::spawn_blocking(move || {
            dispatcher::with_default(&dispatch, || {
                let mut buff = [0u8; 1024];
                loop {
                    let n = reader.read(&mut buff)?;

                    if n > 0 {
                        out_tx.blocking_send(buff[0..n].to_vec())?;
                    } else {
                        break;
                    }
                }

                Ok::<(), ChildError>(())
            })
        });

        let reader_async_bridge = tokio::task::spawn(async move {
            while let Some(buff) = out_rx.recv().await
                && buff.len() > 0
            {
                out_writer.write_all(&buff).await?;
            }
            Ok::<(), ChildError>(())
        });

        let writer_task = tokio::task::spawn(async move {
            let mut buff = [0u8; 1024];
            loop {
                let n = in_reader.read(&mut buff).await?;

                if n > 0 {
                    writer.write_all(&buff[..n])?;
                } else {
                    break;
                }
            }

            Ok::<(), ChildError>(())
        });

        Ok(Self::new(
            ChildInner::Pty {
                child,
                reader_task,
                reader_async_bridge_task: reader_async_bridge,
                writer_task,
            },
            Cell::new(Some(Box::pin(in_writer))),
            Cell::new(Some(Box::pin(out_reader))),
            Cell::new(Some(None)),
            pid,
        ))
    }

    fn spawn_normal(mut cmd: StdCommand) -> Result<Self, ChildError> {
        trace::trace!("spawning normal");
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
                    setsid().inspect_err(|e| {
                        tracing::debug!(
                            "failed to create child process group: {e}"
                        )
                    })?;
                    trace::debug!("child process group created");
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
            Cell::new(Some(Box::pin(stdin))),
            Cell::new(Some(Box::pin(stdout))),
            Cell::new(Some(Some(Box::pin(stderr)))),
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

    pub fn take_error_reader(
        &self,
    ) -> Option<Option<impl ChildOutputReader + use<>>> {
        self.error.take()
    }
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    #[tracing::instrument(skip_all)]
    pub async fn wait(self) -> Result<u32, ChildError> {
        match self.inner {
            ChildInner::Pty {
                mut child,
                reader_task,
                reader_async_bridge_task,
                writer_task,
            } => {
                let dispatch = dispatcher::get_default(|d| d.clone());
                let status = tokio::task::spawn_blocking(move || {
                    dispatcher::with_default(&dispatch, || child.wait())
                });
                let (reader, reader_async_bridge, writer, status) = tokio::try_join!(
                    reader_task,
                    reader_async_bridge_task,
                    writer_task,
                    status
                )?;
                reader?;
                reader_async_bridge?;
                writer?;
                let status = status?;

                trace::trace!("child exited with status: {status:?}");

                Ok(status.exit_code())
            }
            ChildInner::Normal(mut child) => {
                let status = child.wait().await.inspect_err(|e| {
                    trace::error!("wait error: {e}");
                })?;

                trace::trace!("child exited with status: {status:?}");

                Ok(status.code().unwrap_or(0) as u32)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ChildError(pub(crate) ChildErrorInner);

impl ChildError {
    #[allow(unused)]
    pub fn kind(&self) -> ChildErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ChildErrorInner>> From<T> for ChildError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ChildErrorKind), vis(pub), repr(u8))]
#[allow(clippy::enum_variant_names)]
pub(crate) enum ChildErrorInner {
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

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Send(#[from] tokio::sync::mpsc::error::SendError<Vec<u8>>),
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
    Pty {
        child: Box<dyn portable_pty::Child + Send + Sync>,
        reader_task: JoinHandle<Result<(), ChildError>>,
        reader_async_bridge_task: JoinHandle<Result<(), ChildError>>,
        writer_task: JoinHandle<Result<(), ChildError>>,
    },
    Normal(tokio::process::Child),
}
