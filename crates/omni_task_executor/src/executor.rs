use std::io::Read;

use derive_new::new;
use omni_cache::impls::LocalTaskExecutionCacheStoreError;
use omni_context::LoadedContext;
use omni_core::{ProjectGraphError, TaskExecutionGraphError};
use omni_execution_plan::{
    ExecutionPlanProvider as _, ExecutionPlanProviderError,
};
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
    execution_plan_provider::ContextExecutionPlanProvider,
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

        let pipeline = ExecutionPipeline::new(plan, self.context, &self.config);

        let results = if self.config.ui().is_tui() {
            let (temp, file_write_task, sub) =
                self.prepare_in_memory_subscriber()?;

            let result = pipeline.run().with_subscriber(sub).await?;

            file_write_task.await??;

            let file = std::fs::OpenOptions::new().read(true).open(temp)?;

            std::io::copy(&mut file.take(u64::MAX), &mut std::io::stdout())?;

            result
        } else {
            pipeline.run().await?
        };

        trace::info!("Overrall execution time: {:?}", start_time.elapsed());

        Ok(results)
    }

    fn prepare_in_memory_subscriber(
        &self,
    ) -> Result<
        (
            std::path::PathBuf,
            tokio::task::JoinHandle<Result<(), std::io::Error>>,
            omni_tracing_subscriber::TracingSubscriber,
        ),
        TaskExecutorError,
    > {
        let stdout_trace_level =
            self.context.tracing_config().stdout_trace_level;
        let config = TracingConfig {
            stderr_trace_enabled: false,
            stdout_trace_level: TraceLevel::Off,
            ..self.context.tracing_config().clone()
        };
        trace::debug!(
            ?config,
            "updated tracing config for in-memory subscriber"
        );

        let (tx, rx) = crossbeam_channel::bounded::<Vec<u8>>(1024);
        let trace_dir = self.context.trace_dir();
        let temp = trace_dir.join("temp.log");
        let f_temp = temp.clone();
        let file_write_task = tokio::task::spawn_blocking(move || {
            if !trace_dir.exists() {
                std::fs::create_dir_all(&trace_dir)?;
            }

            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(f_temp)?;

            for chunk in rx {
                std::io::copy(&mut chunk.take(u64::MAX), &mut file)?;
            }
            Ok::<_, std::io::Error>(())
        });
        let tracer = InMemoryTracer::new(tx);
        let custom_output = CustomOutput::new_instance(
            CustomOutputConfig {
                trace_level: stdout_trace_level,
                output_type: OutputType::new_text(FormatOptions::default()),
            },
            tracer,
        );
        let sub = omni_tracing_subscriber::TracingSubscriber::new(
            &config,
            vec![custom_output],
        )?;
        Ok((temp, file_write_task, sub))
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
