use std::io::Read;

use derive_new::new;
use omni_cache::impls::LocalTaskExecutionCacheStoreError;
use omni_context::LoadedContext;
use omni_core::{ProjectGraphError, TaskExecutionGraphError};
use omni_tracing_subscriber::{
    TraceLevel, TracingConfig,
    custom_output::{
        CustomOutput, CustomOutputConfig, FormatOptions, OutputType,
    },
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use trace::instrument::WithSubscriber;

use crate::{
    Call, ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    execution_plan_provider::{
        ContextExecutionPlanProvider, ExecutionPlanProvider,
        ExecutionPlanProviderError,
    },
    in_memory_tracer::InMemoryTracer,
    pipeline::{ExecutionPipeline, ExecutionPipelineError},
};

#[derive(Debug, new)]
pub struct TaskExecutor<'a, TSys: TaskExecutorSys> {
    #[new(into)]
    config: ExecutionConfig,
    context: &'a LoadedContext<TSys>,
}

impl<'a, TSys: TaskExecutorSys> TaskExecutor<'a, TSys> {
    pub async fn run(
        &self,
    ) -> Result<Vec<TaskExecutionResult>, TaskExecutorError> {
        let start_time = std::time::Instant::now();
        if self.config.dry_run() {
            trace::info!(
                "Dry run mode enabled, no command execution, cache recording, and cache replay will be performed"
            );
        }

        let plan = ContextExecutionPlanProvider::new(self.context)
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

        let pipeline = ExecutionPipeline::new(plan, self.context, &self.config);

        let mut results = if self.config.ui().is_tui() {
            let stdout_trace_level =
                self.context.tracing_config().stdout_trace_level;
            let config = TracingConfig {
                stderr_trace_enabled: false,
                stdout_trace_level: TraceLevel::None,
                ..self.context.tracing_config().clone()
            };

            let (tx, rx) = crossbeam_channel::bounded::<Vec<u8>>(1024);

            let temp = self.context.trace_dir().join("temp.log");
            let f_temp = temp.clone();
            let file_write_task = tokio::task::spawn_blocking(move || {
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(f_temp)?;

                for chunk in rx {
                    std::io::copy(&mut chunk.take(u64::MAX), &mut file)?;
                }
                Ok::<(), std::io::Error>(())
            });

            let tracer = InMemoryTracer::new(tx);

            let custom_output = CustomOutput::new_instance(
                CustomOutputConfig {
                    trace_level: stdout_trace_level,
                    output_type: OutputType::new_text(FormatOptions::default()),
                },
                tracer,
            );

            let sub = omni_tracing_subscriber::TracerSubscriber::new(
                &config,
                vec![custom_output],
            )?;

            let result = pipeline.run().with_subscriber(sub).await?;

            file_write_task.await??;

            let file = std::fs::OpenOptions::new().read(true).open(temp)?;

            std::io::copy(&mut file.take(u64::MAX), &mut std::io::stdout())?;

            result
        } else {
            pipeline.run().await?
        };

        if self.config.add_task_details() {
            for result in results.iter_mut() {
                let task = result.task();
                let mut details = result.details().cloned().unwrap_or_default();

                if details.meta.is_none() {
                    details.meta = (if self.config.call().is_command() {
                        self.context
                            .get_project_meta_config(task.project_name())
                    } else {
                        self.context.get_task_meta_config(
                            task.project_name(),
                            task.task_name(),
                        )
                    })
                    .cloned();
                }

                result.set_details(details);
            }
        }

        trace::info!("Overrall execution time: {:?}", start_time.elapsed());

        Ok(results)
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

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
}
