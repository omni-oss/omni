use std::time::Duration;

use omni_cache::impls::LocalTaskExecutionCacheStoreError;
use omni_context::LoadedContext;
use omni_core::{ProjectGraphError, TaskExecutionGraphError};
use omni_execution_plan::{
    ExecutionPlanProvider as _, ExecutionPlanProviderError, MetaErrorOptions,
    MetaFilterErrorLimits, MetaFilterExcluded,
};
use omni_messages::{
    ExecutionCompleteEvent, ExecutionEventSubscriber, ExecutionPlanReadyEvent,
    PlanRedactMode, TracingSubscriber,
    publish::{diagnostic_notice, notice},
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use derive_new::new;

use crate::{
    Call, ExecutionConfig, TaskExecutionResult, TaskExecutorSys,
    execution_plan_provider::ContextExecutionPlanProvider,
    notices::{CacheDependencyDemoted, RedactDisabled},
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

        let redact_mode = self.config.redact().mode.unwrap_or(
            self.context
                .workspace_configuration()
                .redact
                .mode
                .unwrap_or_default(),
        );
        if redact_mode == omni_configurations::RedactMode::None {
            notice!(self.subscriber, RedactDisabled);
        }

        let provider = ContextExecutionPlanProvider::new(self.context)
            .with_meta_error_options(MetaErrorOptions {
                on_error: self.config.meta_on_error(),
                limits: MetaFilterErrorLimits::with_samples(
                    self.config.meta_error_samples(),
                ),
            });
        let plan = provider.get_execution_plan(
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

        for excluded in provider.take_excluded_meta_errors() {
            diagnostic_notice!(
                self.subscriber,
                MetaFilterExcluded {
                    filter: excluded.filter.clone(),
                    count: excluded.total,
                },
                &excluded
            );
        }

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
                dry_run: self.config.dry_run(),
                remote_cache: self
                    .context
                    .remote_cache_configuration()
                    .is_some(),
                redact_mode: match redact_mode {
                    omni_configurations::RedactMode::Declared => {
                        PlanRedactMode::Declared
                    }
                    omni_configurations::RedactMode::All => PlanRedactMode::All,
                    omni_configurations::RedactMode::None => {
                        PlanRedactMode::None
                    }
                },
            })
            .await;

        let pipeline = ExecutionPipeline::new(
            plan,
            self.context,
            &self.config,
            &self.subscriber,
        );

        let results = pipeline.run().await?;

        let demoted = results
            .iter()
            .filter(|r| r.cache_status().is_some_and(|s| s.is_demoted()))
            .count();
        if demoted > 0 {
            notice!(self.subscriber, CacheDependencyDemoted { count: demoted });
        }

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

omni_codes::prefixes!("executor");

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct TaskExecutorError(TaskExecutorErrorInner);

impl TaskExecutorError {
    /// The aggregated meta filter failure, if that is what went wrong.
    pub fn meta_filter_error(
        &self,
    ) -> Option<&omni_execution_plan::MetaFilterError> {
        match &self.0 {
            TaskExecutorErrorInner::ExecutionPlanProvider(e) => {
                e.meta_filter_error()
            }
            _ => None,
        }
    }

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

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, new, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(TaskExecutorErrorKind), vis(pub))]
enum TaskExecutorErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    ExecutionPipeline(#[from] ExecutionPipelineError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ExecutionPlanProvider(#[from] ExecutionPlanProviderError),

    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    TaskExecutionGraph(#[from] TaskExecutionGraphError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ProjectGraph(#[from] ProjectGraphError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    #[diagnostic(delegate)]
    LocalTaskExecutionCacheStore(#[from] LocalTaskExecutionCacheStoreError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    MetaFilter(#[from] omni_expressions::Error),

    /// No task matched the call.
    ///
    /// The project, directory and meta filters together selected no task to
    /// run.
    #[error(
        "no task to execute, nothing matches the call: {call} \nproject filters: {project_filters:?}, \ndir filters: {dir_filters:?}, \nmeta filter: {meta_filter:?}"
    )]
    #[diagnostic(
        code = "executor.nothing_to_execute",
        hint = "check the project, directory and meta filters, and that the task is defined in a matching project"
    )]
    NothingToExecute {
        call: Call,
        #[diagnostic(detail)]
        project_filters: Vec<String>,
        #[diagnostic(detail = "dir_filters")]
        dir_filters: Vec<String>,
        #[diagnostic(detail = "meta_filter")]
        meta_filter: Option<String>,
    },

    #[error(transparent)]
    #[diagnostic(skip)]
    Join(#[from] tokio::task::JoinError),
}

#[cfg(test)]
mod diagnostic_tests {
    use eyre::WrapErr;
    use omni_diagnostic::{Audience, find};
    use omni_execution_plan::Call;

    use super::*;

    fn nothing_to_execute() -> TaskExecutorError {
        TaskExecutorErrorInner::new_nothing_to_execute(
            Call::Tasks(vec!["build".into()]),
            vec!["web".into()],
            vec![],
            Some("meta.type == 'lib'".into()),
        )
        .into()
    }

    #[test]
    fn nothing_to_execute_is_a_user_diagnostic() {
        let r = Err::<(), _>(nothing_to_execute())
            .wrap_err("running tasks")
            .unwrap_err();
        let d = find(&r).expect("diagnostic");
        assert_eq!(d.code(), "executor.nothing_to_execute");
        assert_eq!(d.audience(), Audience::User);
        let details = d.details().unwrap();
        assert_eq!(details["project_filters"], serde_json::json!(["web"]));
        assert_eq!(details["meta_filter"], "meta.type == 'lib'");
        assert!(details.get("call").is_none());
        assert_eq!(d.hints().len(), 1);
    }

    #[test]
    fn invalid_shell_syntax_surfaces_through_batch_pipeline_and_executor() {
        use crate::{
            batch_executor::BatchExecutorError,
            pipeline::ExecutionPipelineError,
        };

        let batch = BatchExecutorError::from(
            omni_command_config::ResolveError::InvalidShellSyntax,
        );
        let err: TaskExecutorError = ExecutionPipelineError::from(batch).into();
        let r = Err::<(), _>(err).wrap_err("running tasks").unwrap_err();
        let d = find(&r).expect("diagnostic through all three wrappers");
        assert_eq!(d.code(), "command.invalid_shell_syntax");
        assert_eq!(d.audience(), Audience::User);
        assert!(!d.hints().is_empty());
        assert_eq!(
            omni_diagnostic::DiagnosticReport::from_report(&r)
                .unwrap()
                .exit_code(),
            2
        );
    }

    #[test]
    fn batch_failures_with_a_diagnostic_in_their_cause_are_found() {
        use crate::{
            batch_executor::{BatchExecutorError, BatchExecutorErrorInner},
            pipeline::ExecutionPipelineError,
        };

        let coded: eyre::Report =
            omni_command_config::ResolveError::EmptyArgv.into();
        let batch = BatchExecutorError::from(
            BatchExecutorErrorInner::new_cant_get_task_contexts(coded),
        );
        let err: TaskExecutorError = ExecutionPipelineError::from(batch).into();
        assert_eq!(
            find(&eyre::Report::new(err)).unwrap().code(),
            "command.empty_argv"
        );

        let plain = BatchExecutorError::from(
            BatchExecutorErrorInner::new_cant_get_task_contexts(eyre::eyre!(
                "boom"
            )),
        );
        let err: TaskExecutorError = ExecutionPipelineError::from(plain).into();
        assert!(find(&eyre::Report::new(err)).is_none());
    }

    #[test]
    fn cache_store_and_cache_manager_diagnostics_are_found() {
        use crate::cache_manager::{CacheManagerError, CacheManagerErrorInner};
        use omni_cache::impls::LocalTaskExecutionCacheStoreError;
        use omni_core::ProjectGraphError;
        use omni_task_context::TaskContextProviderError;

        let coded = || eyre::Report::new(ProjectGraphError::not_found("web"));

        let store = LocalTaskExecutionCacheStoreError::from(
            TaskContextProviderError::from(coded()),
        );
        let err: TaskExecutorError = store.into();
        assert_eq!(
            find(&eyre::Report::new(err)).unwrap().code(),
            "project.not_found"
        );

        let manager =
            CacheManagerError::from(CacheManagerErrorInner::GetCacheFailed {
                source: coded(),
            });
        assert_eq!(
            find(&eyre::Report::new(manager)).unwrap().code(),
            "project.not_found"
        );
    }

    #[test]
    fn plain_executor_errors_stay_uncoded() {
        let plain: TaskExecutorError = eyre::eyre!("plain").into();
        assert!(find(&eyre::Report::new(plain)).is_none());

        let io: TaskExecutorError = std::io::Error::other("x").into();
        assert!(find(&eyre::Report::new(io)).is_none());
    }

    #[test]
    fn code_is_registered_in_the_catalog() {
        assert!(omni_codes::get("executor.nothing_to_execute").is_some());
        if let Err(errors) = omni_codes::validate() {
            let msgs: Vec<String> =
                errors.iter().map(|e| e.to_string()).collect();
            panic!("invalid code registry:\n{}", msgs.join("\n"));
        }
    }
}
