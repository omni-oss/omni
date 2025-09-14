use derive_new::new;
use maps::unordered_map;
use omni_cache::impls::LocalTaskExecutionCacheStore;
use omni_context::LoadedContext;
use omni_core::BatchedExecutionPlan;
use omni_term_ui::mux_output_presenter::{
    MuxOutputPresenter, MuxOutputPresenterError, MuxOutputPresenterStatic,
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    batch_executor::{BatchExecutor, BatchExecutorError},
    cache_manager::CacheManagerBuilder,
    cache_store_provider::{CacheStoreProvider, ContextCacheStoreProvider},
    task_context_provider::ContextTaskContextProvider,
};

#[derive(Debug, new)]
pub struct ExecutionPipeline<'a, TSys: TaskExecutorSys> {
    plan: BatchedExecutionPlan,
    context: &'a LoadedContext<TSys>,
    config: &'a ExecutionConfig,
}

impl<'a, TSys: TaskExecutorSys> ExecutionPipeline<'a, TSys> {
    pub async fn run(
        self,
    ) -> Result<Vec<TaskExecutionResult>, ExecutionPipelineError> {
        let execution_plan = self.plan;

        let task_count: usize = execution_plan.iter().map(|b| b.len()).sum();

        let cache_store =
            ContextCacheStoreProvider::new(self.context).get_cache_store();

        let cache_manager =
            CacheManagerBuilder::<LocalTaskExecutionCacheStore>::default()
                .store(cache_store)
                .dry_run(self.config.dry_run())
                .force(self.config.force())
                .no_cache(self.config.no_cache())
                .build()
                .expect("should be able to create cache manager");

        let mut results_accumulator = unordered_map!(cap: task_count);

        let task_context_provider =
            ContextTaskContextProvider::new(self.context);

        let presenter = match self.config.ui() {
            omni_configurations::Ui::Stream => {
                MuxOutputPresenterStatic::new_stream()
            }
            omni_configurations::Ui::Tui => MuxOutputPresenterStatic::new_tui(),
        };

        let mut batch_exec = BatchExecutor::new(
            task_context_provider,
            cache_manager,
            self.context.sys().clone(),
            &presenter,
            self.config.max_concurrency().unwrap_or(num_cpus::get() * 4),
            self.config.ignore_dependencies(),
            self.config.on_failure(),
            self.config.dry_run(),
            self.config.replay_cached_logs(),
        );

        for batch in &execution_plan {
            let results = batch_exec
                .execute_batch(&batch, &results_accumulator)
                .await?;

            results_accumulator.extend(results);
        }

        presenter.close().await?;

        Ok(results_accumulator.into_values().collect())
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
    #[allow(unused)]
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
enum ExecutionPipelineErrorInner {
    #[error(transparent)]
    BatchExecutor(#[from] BatchExecutorError),

    #[error(transparent)]
    MuxOutputPresenter(#[from] MuxOutputPresenterError),
}
