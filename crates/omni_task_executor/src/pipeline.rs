use omni_cache::impls::HybridTaskExecutionCacheStore;
use omni_context::LoadedContext;
use omni_core::BatchedExecutionPlan;
use omni_messages::{
    ExecutionEventSubscriber,
    execution::events::{BatchCompletedEvent, BatchStartEvent},
};
use omni_types::TaskMap;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{
    ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    batch_executor::{BatchExecutor, BatchExecutorError},
    cache_manager::{CacheManager, local_only_set},
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

        // The signing key is for omni's own uploads: no task needs it.
        if let Some(rc) = self.context.remote_cache_configuration()
            && let Some(key) = &rc.signing_key
            && let Some(name) = &key.private_key_env
        {
            omni_process::protect_env_var(name);
        }

        let store_provider = ContextCacheStoreProvider::new(self.context);
        let cache_store = store_provider.get_cache_store();

        // With remote caching off everything is local already. Otherwise the
        // tasks that opted out (or all of them under `--no-remote-cache`) and
        // everything they depend on run against a store that never talks to the
        // remote, decided once before the first batch.
        let remote_enabled =
            self.context.remote_cache_configuration().is_some();
        let local_only = local_only_set(
            execution_plan.iter().flatten(),
            remote_enabled,
            |project, task| {
                self.context
                    .get_cache_info(project, task)
                    .is_some_and(|info| !info.cache_remote)
            },
            self.config.no_remote_cache(),
            self.config.ignore_dependencies(),
        );

        let cache_manager =
            CacheManager::<HybridTaskExecutionCacheStore, TSys>::builder()
                .store(cache_store)
                .maybe_local_store(
                    (!local_only.is_empty())
                        .then(|| store_provider.get_local_cache_store()),
                )
                .local_only(local_only)
                .dry_run(self.config.dry_run())
                .force(self.config.force())
                .no_cache(self.config.no_cache())
                .root_dir(self.context.root_dir().to_path_buf())
                .cache_dir(self.context.cache_dir())
                .sys(self.context.sys().clone())
                .build();

        let mut results_accumulator: TaskMap<TaskExecutionResult> =
            TaskMap::new();

        let mut batch_exec = BatchExecutor::new(
            self.context,
            cache_manager,
            self.subscriber,
            self.config.max_concurrency().unwrap_or(num_cpus::get() * 4),
            self.config.ignore_dependencies(),
            self.config.on_failure(),
            self.config.dry_run(),
            self.config.output_logs(),
            self.config.output_cached_logs(),
            self.config.retry().clone(),
            self.config.no_cache(),
            self.config.add_task_details(),
            self.config.args(),
            self.config.redact(),
        );

        for batch in &execution_plan {
            self.subscriber.on_batch_start(BatchStartEvent {}).await;

            let results = batch_exec
                .execute_batch(batch, &results_accumulator)
                .await?;

            self.subscriber
                .on_batch_completed(BatchCompletedEvent {})
                .await;

            results_accumulator.extend(results);
        }

        Ok(ordered_results(results_accumulator))
    }
}

fn ordered_results<V>(results: TaskMap<V>) -> Vec<V> {
    results.into_sorted_values()
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(ExecutionPipelineErrorKind), vis(pub))]
enum ExecutionPipelineErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    BatchExecutor(#[from] BatchExecutorError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_ordered_by_project_then_task() {
        const PROJECTS: usize = 100;
        const TASKS: [&str; 3] = ["build", "lint", "test"];
        const RUNS: usize = 20;

        let mut expected = Vec::new();
        for p in 0..PROJECTS {
            for t in TASKS {
                expected.push((format!("project-{p}"), t.to_string()));
            }
        }
        expected.sort();

        // Every `TaskMap` seeds its hashers independently, so each run starts
        // from a different iteration order. A fixed order surviving
        // `RUNS` x 300 entries by luck is negligible.
        for run in 0..RUNS {
            let mut map = TaskMap::new();
            for (project, task) in expected.iter().rev() {
                map.insert(project, task, (project.clone(), task.clone()));
            }

            assert!(
                ordered_results(map) == expected,
                "run {run}: results are not sorted by (project, task)"
            );
        }
    }
}
