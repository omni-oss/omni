use std::time::Duration;

use omni_cache::impls::LocalTaskExecutionCacheStoreError;
use omni_context::LoadedContext;
use omni_core::{ProjectGraphError, TaskExecutionGraphError};
use omni_execution_plan::{
    ExecutionPlanProvider as _, ExecutionPlanProviderError,
};
use omni_messages::{
    DiagnosticLevel, ExecutionCompleteEvent, ExecutionEventSubscriber,
    ExecutionPlanReadyEvent, TracingSubscriber, diagnostic,
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use derive_new::new;

use crate::{
    Call, ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    execution_plan_provider::ContextExecutionPlanProvider,
    pipeline::{ExecutionPipeline, ExecutionPipelineError},
};

pub struct TaskExecutor<
    'a,
    TSys: TaskExecutorSys,
    S: ExecutionEventSubscriber = TracingSubscriber,
> {
    config: ExecutionConfig,
    context: &'a LoadedContext<TSys>,
    subscriber: S,
}

impl<'a, TSys: TaskExecutorSys, S: ExecutionEventSubscriber>
    TaskExecutor<'a, TSys, S>
{
    pub fn new(
        config: impl Into<ExecutionConfig>,
        context: &'a LoadedContext<TSys>,
        subscriber: S,
    ) -> Self {
        Self {
            config: config.into(),
            context,
            subscriber,
        }
    }

    pub async fn run(
        &self,
    ) -> Result<Vec<TaskExecutionResult>, TaskExecutorError> {
        let start_time = std::time::Instant::now();

        if self.config.dry_run() {
            diagnostic!(
                self.subscriber,
                DiagnosticLevel::Info,
                "Dry run mode enabled, no command execution, cache recording, and cache replay will be performed",
            ).await;
        }

        let plan = ContextExecutionPlanProvider::new(self.context)
            .get_execution_plan(
                self.config.call(),
                self.config
                    .project_filters()
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .as_slice(),
                self.config
                    .dir_filters()
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .as_slice(),
                self.config.meta_filter().as_deref(),
                self.config.scm_affected_filter().as_ref(),
                self.config.ignore_dependencies(),
                self.config.with_dependents(),
            )?;

        let empty = plan.is_empty() || plan.iter().all(|b| b.is_empty());

        if empty {
            Err(TaskExecutorErrorInner::new_nothing_to_execute(
                self.config.call().clone(),
                self.config
                    .project_filters()
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
                self.config
                    .dir_filters()
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
                self.config.meta_filter().clone(),
            ))?;
        }

        self.subscriber
            .on_execution_plan_ready(ExecutionPlanReadyEvent {
                total: plan.iter().flatten().count(),
                has_interactive_or_persistent_tasks: plan
                    .iter()
                    .flatten()
                    .any(|t| t.interactive() || t.persistent()),
            })
            .await;

        let pipeline = ExecutionPipeline::new(
            plan,
            self.context,
            &self.config,
            &self.subscriber,
        );

        let results = pipeline.run().await?;

        let total_time_saved: Duration = results
            .iter()
            .filter_map(|r| match r {
                TaskExecutionResult::Completed {
                    elapsed,
                    cache_hit: true,
                    ..
                } => Some(*elapsed),
                _ => None,
            })
            .sum();

        self.subscriber
            .on_execution_complete(ExecutionCompleteEvent {
                total: results.len(),
                succeeded: results.iter().filter(|r| r.success()).count(),
                failed: results
                    .iter()
                    .filter(|r| !r.is_skipped() && !r.success())
                    .count(),
                skipped: results.iter().filter(|r| r.is_skipped()).count(),
                cache_hits: results
                    .iter()
                    .filter(|r| {
                        matches!(
                            r,
                            crate::TaskExecutionResult::Completed {
                                cache_hit: true,
                                ..
                            }
                        )
                    })
                    .count(),
                elapsed: start_time.elapsed(),
                total_time_saved,
            })
            .await;

        Ok(results)
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct TaskExecutorError(TaskExecutorErrorInner);

impl TaskExecutorError {
    pub fn kind(&self) -> TaskExecutorErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<TaskExecutorErrorInner>> From<T> for TaskExecutorError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
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

    #[error(
        "no task to execute, nothing matches the call: {call} \nproject filters: {project_filters:?}, \ndir filters: {dir_filters:?}, \nmeta filter: {meta_filter:?}"
    )]
    NothingToExecute {
        call: Call,
        project_filters: Vec<String>,
        dir_filters: Vec<String>,
        meta_filter: Option<String>,
    },

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
}
