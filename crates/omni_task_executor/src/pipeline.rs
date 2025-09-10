use std::collections::HashMap;

use derive_new::new;
use omni_cache::impls::LocalTaskExecutionCacheStore;
use omni_context::LoadedContext;
use omni_core::BatchedExecutionPlan;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    cache_manager::CacheManagerBuilder,
    cache_store_provider::{CacheStoreProvider, ContextCacheStoreProvider},
};

#[derive(Debug, new)]
pub struct ExecutionPipeline {
    plan: BatchedExecutionPlan,
}

impl ExecutionPipeline {
    pub async fn run<TSys: TaskExecutorSys>(
        self,
        context: &LoadedContext<TSys>,
        config: &ExecutionConfig,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionPipelineError> {
        let execution_plan = self.plan;

        let task_count: usize = execution_plan.iter().map(|b| b.len()).sum();

        let mut overall_results =
            HashMap::<String, TaskExecutionResult>::with_capacity(task_count);

        let cache_store =
            ContextCacheStoreProvider::new(context).get_cache_store();

        let cache_manager =
            CacheManagerBuilder::<LocalTaskExecutionCacheStore>::default()
                .store(cache_store)
                .dry_run(config.dry_run())
                .force(config.force())
                .no_cache(config.no_cache())
                .build()
                .expect("should be able to create cache manager");

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
