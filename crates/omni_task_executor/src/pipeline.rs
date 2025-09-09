use derive_new::new;
use omni_context::LoadedContext;
use omni_core::BatchedExecutionPlan;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{TaskExecutionResult, TaskExecutorSys};

#[derive(Debug, new)]
pub struct ExecutionPipeline {
    plan: BatchedExecutionPlan,
}

impl ExecutionPipeline {
    pub async fn run<TSys: TaskExecutorSys>(
        self,
        context: &LoadedContext<TSys>,
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
