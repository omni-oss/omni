use std::{
    collections::HashMap, ffi::OsString, path::PathBuf, process::ExitStatus,
    sync::Arc,
};

use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use system_traits::impls::RealSys;
use tokio::sync::Mutex;

use crate::{
    context::{Context, ContextSys},
    executor::{CommandExecutor, CommandExecutorError},
};

#[derive(Debug, new)]
pub struct TaskExecutor<TContextSys: ContextSys = RealSys> {
    #[new(into)]
    command: String,

    #[new(into)]
    cwd: PathBuf,

    #[new(into)]
    context: Arc<Mutex<Context<TContextSys>>>,
}

impl TaskExecutor {
    pub async fn run(self) -> Result<ExitStatus, TaskExecutorError> {
        let env = self
            .context
            .lock()
            .await
            .get_cached_env_vars(&self.cwd)?
            .iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect::<HashMap<OsString, OsString>>();

        let executor =
            CommandExecutor::from_comand_and_env(self.command, self.cwd, env)?;

        let _stdout = executor
            .take_stderr()
            .expect("should have stderr at this point");
        let _stderr = executor
            .take_stdout()
            .expect("should have stdout at this point");
        let _stdin = executor
            .take_stdin()
            .expect("should have stdin at this point");

        Ok(executor.run().await?)
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

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(TaskExecutorErrorKind), vis(pub), repr(u8))]
enum TaskExecutorErrorInner {
    #[error("can't run command: {0}")]
    CantRunCommand(#[from] CommandExecutorError),

    #[error("can't get env vars: {0}")]
    CantGetEnvVars(#[from] eyre::Report),
}
