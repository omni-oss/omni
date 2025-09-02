use omni_cache::impls::LocalTaskExecutionCacheStoreError;
use omni_core::{ProjectGraphError, TaskExecutionGraphError};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::Call;

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
#[strum_discriminants(name(TaskExecutorErrorKind), vis(pub))]
enum TaskExecutorErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    // #[error(transparent)]
    // CantGetEnvVars(eyre::Report),
    #[error("task is empty")]
    TaskIsEmpty,

    // #[error("command is empty")]
    // CommandIsEmpty,

    // #[error("task '{task}' not found")]
    // TaskNotFound { task: String },
    #[error("no project found for criteria: filter = '{filter}'")]
    NoProjectFound { filter: String },

    #[error("no task to execute: {0} not found")]
    NothingToExecute(Call),

    #[error(transparent)]
    TaskExecutionGraph(#[from] TaskExecutionGraphError),

    #[error(transparent)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    LocalTaskExecutionCacheStore(#[from] LocalTaskExecutionCacheStoreError),

    #[error(transparent)]
    MetaFilter(#[from] omni_expressions::Error),
}
