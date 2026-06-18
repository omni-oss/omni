/// `TracingSubscriber` translates events into `tracing` calls.
///
/// - [`DiagnosticEvent`] → `tracing::info!/warn!/error!/debug!/trace!`
/// - Lifecycle events → `tracing::debug!` with structured fields
/// - `subscription_config().wants_task_output_stream == false` — no byte
///   streams are created, saving a pipe allocation per task.
///
/// This is the default subscriber used by `OmniApi` when the caller provides
/// no explicit subscriber.
///
/// [`DiagnosticEvent`]: crate::diagnostic::DiagnosticEvent
#[cfg(feature = "tracing")]
pub use tracing_impl::TracingSubscriber;

#[cfg(feature = "tracing")]
mod tracing_impl {
    use ::tracing as t;

    use crate::diagnostic::{
        DiagnosticEvent, DiagnosticLevel, DiagnosticSubscriber,
    };
    use crate::execution::{
        CacheHitEvent, ExecutionCompleteEvent, ExecutionEventSubscriber,
        ExecutionPlanReadyEvent, TaskCompletedEvent, TaskFailedEvent,
        TaskRetryingEvent, TaskSkippedEvent, TaskStartedEvent,
    };
    use crate::generator::{
        GeneratorCompletedEvent, GeneratorEventSubscriber,
        GeneratorFileCreatedEvent, GeneratorFileSkippedEvent,
        GeneratorStartEvent,
    };

    pub struct TracingSubscriber;

    impl DiagnosticSubscriber for TracingSubscriber {
        async fn on_diagnostic(&self, e: DiagnosticEvent) {
            let msg = e.message.as_str();
            let target = e.target.as_str();
            match e.level {
                DiagnosticLevel::Trace => t::trace!(target = target, "{msg}"),
                DiagnosticLevel::Debug => t::debug!(target = target, "{msg}"),
                DiagnosticLevel::Info => t::info!(target = target, "{msg}"),
                DiagnosticLevel::Warn => t::warn!(target = target, "{msg}"),
                DiagnosticLevel::Error => t::error!(target = target, "{msg}"),
            }
        }
    }

    impl ExecutionEventSubscriber for TracingSubscriber {
        fn wants_task_output_stream(&self) -> bool {
            // TracingSubscriber only cares about structured events, not raw bytes.
            false
        }

        async fn on_task_started(&self, e: TaskStartedEvent) {
            t::debug!(task_id = %e.task_id, project = %e.project, task = %e.task, "task_started");
        }
        async fn on_execution_plan_ready(&self, e: ExecutionPlanReadyEvent) {
            t::debug!(
                total = e.total,
                has_interactive_or_persistent_tasks =
                    e.has_interactive_or_persistent_tasks,
                "execution_plan_ready"
            );
        }
        async fn on_task_completed(&self, e: TaskCompletedEvent) {
            t::debug!(
                task_id = %e.task_id,
                exit_code = e.exit_code,
                elapsed_ms = e.elapsed.as_millis(),
                cache_hit = e.cache_hit,
                tries = e.tries,
                "task_completed"
            );
        }
        async fn on_task_failed(&self, e: TaskFailedEvent) {
            t::debug!(task_id = %e.task_id, error = %e.error, tries = e.tries, "task_failed");
        }
        async fn on_task_skipped(&self, e: TaskSkippedEvent) {
            t::debug!(task_id = %e.task_id, reason = %e.reason, "task_skipped");
        }
        async fn on_task_retrying(&self, e: TaskRetryingEvent) {
            t::debug!(
                task_id = %e.task_id,
                attempt = e.attempt,
                max_retries = e.max_retries,
                "task_retrying"
            );
        }
        async fn on_cache_hit(&self, e: CacheHitEvent) {
            t::debug!(task_id = %e.task_id, "cache_hit");
        }
        async fn on_execution_complete(&self, e: ExecutionCompleteEvent) {
            t::info!(
                total = e.total,
                succeeded = e.succeeded,
                failed = e.failed,
                skipped = e.skipped,
                cache_hits = e.cache_hits,
                elapsed_ms = e.elapsed.as_millis(),
                "execution_complete"
            );
        }
    }

    impl GeneratorEventSubscriber for TracingSubscriber {
        async fn on_generator_start(&self, e: GeneratorStartEvent) {
            t::debug!(name = %e.name, "generator_started");
        }
        async fn on_file_created(&self, e: GeneratorFileCreatedEvent) {
            t::debug!(generator = %e.generator, path = ?e.path, "generator_file_created");
        }
        async fn on_file_skipped(&self, e: GeneratorFileSkippedEvent) {
            t::debug!(generator = %e.generator, path = ?e.path, reason = %e.reason, "generator_file_skipped");
        }
        async fn on_generator_completed(&self, e: GeneratorCompletedEvent) {
            t::debug!(name = %e.name, "generator_complete");
        }
    }
}

#[cfg(all(test, feature = "tracing"))]
mod tests {
    use super::tracing_impl::TracingSubscriber;
    use crate::{
        DiagnosticEvent, DiagnosticLevel, ExecutionCompleteEvent,
        TaskCompletedEvent, TaskStartedEvent, diagnostic::DiagnosticSubscriber,
        execution::ExecutionEventSubscriber,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn tracing_subscriber_does_not_panic_on_lifecycle_events() {
        let sub = TracingSubscriber;

        sub.on_task_started(TaskStartedEvent {
            task_id: "proj::task".into(),
            project: "proj".into(),
            task: "task".into(),
        })
        .await;

        sub.on_task_completed(TaskCompletedEvent {
            task_id: "proj::task".into(),
            project: "proj".into(),
            task: "task".into(),
            exit_code: 0,
            elapsed: Duration::from_millis(10),
            cache_hit: false,
            tries: 1,
        })
        .await;

        sub.on_execution_complete(ExecutionCompleteEvent {
            total: 1,
            succeeded: 1,
            failed: 0,
            skipped: 0,
            cache_hits: 0,
            elapsed: Duration::from_millis(10),
        })
        .await;
    }

    #[tokio::test]
    async fn tracing_subscriber_handles_all_diagnostic_levels() {
        let sub = TracingSubscriber;
        for level in [
            DiagnosticLevel::Trace,
            DiagnosticLevel::Debug,
            DiagnosticLevel::Info,
            DiagnosticLevel::Warn,
            DiagnosticLevel::Error,
        ] {
            sub.on_diagnostic(DiagnosticEvent {
                level,
                message: format!("test at level {level}"),
                fields: Default::default(),
                target: "omni::test".into(),
            })
            .await;
        }
    }

    #[test]
    fn tracing_subscriber_does_not_want_output_stream() {
        use crate::execution::ExecutionEventSubscriber as _;
        assert!(
            !TracingSubscriber.wants_task_output_stream(),
            "TracingSubscriber should not allocate output stream pipes"
        );
        assert!(
            !TracingSubscriber.wants_task_input_stream(),
            "TracingSubscriber should not allocate input stream pipes"
        );
    }

    #[test]
    fn tracing_subscriber_is_zero_sized() {
        assert_eq!(std::mem::size_of::<TracingSubscriber>(), 0);
    }
}
