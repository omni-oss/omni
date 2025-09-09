use derive_new::new;
use omni_cache::impls::LocalTaskExecutionCacheStore;
use omni_context::LoadedContext;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{TaskExecutionResult, TaskExecutorSys};

#[derive(Debug, new)]
pub struct ExecutionPipeline<'a, TSys: TaskExecutorSys> {
    context: &'a LoadedContext<TSys>,
    cache_store: Option<LocalTaskExecutionCacheStore>,
}

impl<'a, TSys: TaskExecutorSys + 'static> ExecutionPipeline<'a, TSys> {
    pub async fn run(
        &mut self,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionPipelineError> {
        todo!("implement pipeline")
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct ExecutionPipelineError {
    #[source]
    inner: ExecutionPipelineErrorInner,
    kind: ExecutionPipelineErrorKind,
}

impl ExecutionPipelineError {
    pub fn kind(&self) -> ExecutionPipelineErrorKind {
        self.kind
    }
}

impl<T: Into<ExecutionPipelineErrorInner>> From<T> for ExecutionPipelineError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ExecutionPipelineErrorKind), vis(pub))]
enum ExecutionPipelineErrorInner {}
