use derive_new::new;
use omni_cache::impls::LocalTaskExecutionCacheStoreError;
use omni_context::LoadedContext;
use omni_core::{ProjectGraphError, TaskExecutionGraphError};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    Call, ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    execution_plan_provider::{
        ContextExecutionPlanProvider, ExecutionPlanProvider,
        ExecutionPlanProviderError,
    },
    pipeline::{ExecutionPipeline, ExecutionPipelineError},
};

#[derive(Debug, new)]
pub struct TaskExecutor {
    #[new(into)]
    config: ExecutionConfig,
}

impl TaskExecutor {
    pub async fn execute<TSys: TaskExecutorSys>(
        &self,
        context: &LoadedContext<TSys>,
    ) -> Result<Vec<TaskExecutionResult>, TaskExecutorError> {
        let plan = ContextExecutionPlanProvider::new(context)
            .get_execution_plan(
                self.config.call(),
                self.config.project_filter().as_deref(),
                self.config.meta_filter().as_deref(),
                self.config.ignore_dependencies(),
            )?;

        let empty = plan.is_empty() || plan.iter().all(|b| b.is_empty());

        if empty {
            Err(TaskExecutorErrorInner::NothingToExecute(
                self.config.call().clone(),
            ))?;
        }

        let pipeline = ExecutionPipeline::new(plan);

        Ok(pipeline.run(context, &self.config).await?)
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
#[strum_discriminants(name(TaskExecutorErrorKind), vis(pub))]
enum TaskExecutorErrorInner {
    #[error(transparent)]
    ExecutionPipeline(#[from] ExecutionPipelineError),

    #[error(transparent)]
    ExecutionPlanProvider(#[from] ExecutionPlanProviderError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

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

    #[error("no task to execute, nothing matches the call: {0}")]
    NothingToExecute(Call),
}
