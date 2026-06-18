use crate::diagnostic::DiagnosticSubscriber;
use crate::execution::events::{BatchCompletedEvent, BatchStartEvent};

use super::events::{
    CacheHitEvent, ExecutionCompleteEvent, ExecutionPlanReadyEvent,
    TaskCompletedEvent, TaskFailedEvent, TaskRetryingEvent, TaskSkippedEvent,
    TaskStartedEvent,
};
use super::stream::TaskOutputStreamEvent;

/// Subscriber for task-execution lifecycle events.
///
/// Requires [`DiagnosticSubscriber`] — implement that trait on your type to
/// receive (or opt out of) diagnostic events.
///
/// All methods are `async` (AFIT — no `#[async_trait]` required for generic
/// usage). All methods take `&self`; use interior mutability when state is
/// needed so that multiple concurrent tasks can call these methods safely.
///
/// The two `wants_*` methods are read once at executor setup to decide whether
/// to allocate pipe infrastructure for each spawned process.
pub trait ExecutionEventSubscriber: DiagnosticSubscriber {
    /// If `false`: task stdout/stderr is NOT piped. No `on_task_output_stream`
    /// call is made, and the executor skips the duplex-pipe alloc entirely
    /// (saves a tokio duplex alloc + copy task per spawned process).
    ///
    /// Defaults to `true`.
    fn wants_task_output_stream(&self) -> bool {
        true
    }

    /// If `false`: no stdin pipe is created even for interactive/persistent tasks.
    ///
    /// Defaults to `false`.
    fn wants_task_input_stream(&self) -> bool {
        false
    }

    async fn on_task_started(&self, _event: TaskStartedEvent) {}

    /// Called once after the execution plan is built, before any task starts.
    /// Subscribers that need to choose between stream and TUI output modes
    /// should make that decision here.
    async fn on_execution_plan_ready(&self, _event: ExecutionPlanReadyEvent) {}

    /// Called only when `wants_task_output_stream() == true`.
    ///
    /// **The subscriber MUST spawn a task to drain `event.stream.reader`.**
    /// Failure to do so will deadlock the child process.
    async fn on_task_output_stream(&self, _event: TaskOutputStreamEvent) {}

    async fn on_task_completed(&self, _event: TaskCompletedEvent) {}
    async fn on_task_failed(&self, _event: TaskFailedEvent) {}
    async fn on_task_skipped(&self, _event: TaskSkippedEvent) {}
    async fn on_task_retrying(&self, _event: TaskRetryingEvent) {}
    async fn on_cache_hit(&self, _event: CacheHitEvent) {}
    async fn on_execution_complete(&self, _event: ExecutionCompleteEvent) {}
    async fn on_batch_start(&self, _event: BatchStartEvent) {}
    async fn on_batch_completed(&self, _event: BatchCompletedEvent) {}
}

/// Forward all subscriber calls through a shared reference.
///
/// This lets `OmniApi` pass `&self.subscriber` into functions that take
/// `S: ExecutionEventSubscriber` by value (e.g. `TaskExecutor::new`).
/// `DiagnosticSubscriber for &S` is covered by the blanket in `diagnostic.rs`.
impl<S: ExecutionEventSubscriber> ExecutionEventSubscriber for &S {
    fn wants_task_output_stream(&self) -> bool {
        S::wants_task_output_stream(*self)
    }
    fn wants_task_input_stream(&self) -> bool {
        S::wants_task_input_stream(*self)
    }
    async fn on_task_started(&self, event: TaskStartedEvent) {
        S::on_task_started(*self, event).await
    }
    async fn on_execution_plan_ready(&self, event: ExecutionPlanReadyEvent) {
        S::on_execution_plan_ready(*self, event).await
    }
    async fn on_task_output_stream(&self, event: TaskOutputStreamEvent) {
        S::on_task_output_stream(*self, event).await
    }
    async fn on_task_completed(&self, event: TaskCompletedEvent) {
        S::on_task_completed(*self, event).await
    }
    async fn on_task_failed(&self, event: TaskFailedEvent) {
        S::on_task_failed(*self, event).await
    }
    async fn on_task_skipped(&self, event: TaskSkippedEvent) {
        S::on_task_skipped(*self, event).await
    }
    async fn on_task_retrying(&self, event: TaskRetryingEvent) {
        S::on_task_retrying(*self, event).await
    }
    async fn on_cache_hit(&self, event: CacheHitEvent) {
        S::on_cache_hit(*self, event).await
    }
    async fn on_execution_complete(&self, event: ExecutionCompleteEvent) {
        S::on_execution_complete(*self, event).await
    }
    async fn on_batch_completed(&self, _event: BatchCompletedEvent) {
        S::on_batch_completed(*self, _event).await
    }
    async fn on_batch_start(&self, _event: BatchStartEvent) {
        S::on_batch_start(*self, _event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoopSubscriber, TaskStartedEvent};

    #[tokio::test]
    async fn ref_impl_forwards_stream_flags() {
        let sub = NoopSubscriber;
        let ref_sub: &NoopSubscriber = &sub;
        assert!(!ref_sub.wants_task_output_stream());
        assert!(!ref_sub.wants_task_input_stream());
    }

    #[tokio::test]
    async fn ref_impl_accepts_events_without_panic() {
        let sub = NoopSubscriber;
        let ref_sub: &NoopSubscriber = &sub;
        ref_sub
            .on_task_started(TaskStartedEvent {
                task_id: "p::t".into(),
                project: "p".into(),
                task: "t".into(),
            })
            .await;
    }

    fn _assert_ref_satisfies_trait<S: ExecutionEventSubscriber>(_: S) {}
    fn _call_site() {
        let sub = NoopSubscriber;
        _assert_ref_satisfies_trait(&sub);
    }
}
