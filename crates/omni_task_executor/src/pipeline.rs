use maps::unordered_map;
use omni_cache::impls::HybridTaskExecutionCacheStore;
use omni_context::LoadedContext;
use omni_core::BatchedExecutionPlan;
use omni_messages::{
    ExecutionEventSubscriber,
    execution::events::{BatchCompletedEvent, BatchStartEvent},
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    batch_executor::{BatchExecutor, BatchExecutorError},
    cache_manager::CacheManager,
    cache_store_provider::{CacheStoreProvider, ContextCacheStoreProvider},
};

pub struct ExecutionPipeline<
    'a,
    TSys: TaskExecutorSys,
    S: ExecutionEventSubscriber,
> {
    plan: BatchedExecutionPlan,
    context: &'a LoadedContext<TSys>,
    config: &'a ExecutionConfig,
    subscriber: &'a S,
}

impl<'a, TSys: TaskExecutorSys, S: ExecutionEventSubscriber>
    ExecutionPipeline<'a, TSys, S>
{
    pub fn new(
        plan: BatchedExecutionPlan,
        context: &'a LoadedContext<TSys>,
        config: &'a ExecutionConfig,
        subscriber: &'a S,
    ) -> Self {
        Self {
            plan,
            context,
            config,
            subscriber,
        }
    }

    pub async fn run(
        self,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionPipelineError> {
        let execution_plan = self.plan;

        let task_count: usize = execution_plan.iter().map(|b| b.len()).sum();

        let cache_store =
            ContextCacheStoreProvider::new(self.context).get_cache_store();

        let cache_manager =
            CacheManager::<HybridTaskExecutionCacheStore, TSys>::builder()
                .store(cache_store)
                .dry_run(self.config.dry_run())
                .force(self.config.force())
                .no_cache(self.config.no_cache())
                .root_dir(self.context.root_dir().to_path_buf())
                .cache_dir(self.context.cache_dir())
                .sys(self.context.sys().clone())
                .build();

        let mut results_accumulator = unordered_map!(cap: task_count);

        if self.context.remote_cache_configuration().is_some() {
            log::info!("Remote caching enabled");
        }

        let mut batch_exec = BatchExecutor::new(
            self.context,
            cache_manager,
            self.context.sys().clone(),
            self.subscriber,
            self.config.max_concurrency().unwrap_or(num_cpus::get() * 4),
            self.config.ignore_dependencies(),
            self.config.on_failure(),
            self.config.dry_run(),
            self.config.output_logs(),
            self.config.output_cached_logs(),
            self.config.max_retries(),
            self.config.retry_interval(),
            self.config.no_cache(),
            self.config.add_task_details(),
            self.config.args(),
        );

        for batch in &execution_plan {
            self.subscriber.on_batch_start(BatchStartEvent {}).await;

            let results = batch_exec
                .execute_batch(&batch, &results_accumulator)
                .await?;

            self.subscriber
                .on_batch_completed(BatchCompletedEvent {})
                .await;

            results_accumulator.extend(results);
        }

        Ok(results_accumulator.into_values().collect())
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ExecutionPipelineError(ExecutionPipelineErrorInner);

impl ExecutionPipelineError {
    #[allow(unused)]
    pub fn kind(&self) -> ExecutionPipelineErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ExecutionPipelineErrorInner>> From<T> for ExecutionPipelineError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ExecutionPipelineErrorKind), vis(pub))]
enum ExecutionPipelineErrorInner {
    #[error(transparent)]
    BatchExecutor(#[from] BatchExecutorError),
}
