use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, unbounded_channel,
};

use crate::diagnostic::{DiagnosticEvent, DiagnosticSubscriber};
use crate::execution::{
    CacheHitEvent, ExecutionCompleteEvent, ExecutionEventSubscriber,
    ExecutionPlanReadyEvent, TaskCompletedEvent, TaskFailedEvent,
    TaskOutputStreamEvent, TaskRetryingEvent, TaskSkippedEvent,
    TaskStartedEvent,
};
use crate::generator::events::{
    GeneratorActionFailedEvent, GeneratorActionInProgressEvent,
    GeneratorActionSkippedEvent, GeneratorActionSuccessEvent,
};
use crate::generator::{
    GeneratorCompletedEvent, GeneratorEventSubscriber,
    GeneratorFileCreatedEvent, GeneratorFileSkippedEvent, GeneratorStartEvent,
};

/// All events that can travel through the channel.
///
/// `TaskOutputStreamEvent` is intentionally excluded — byte streams cannot be
/// sent through an unbounded channel. `on_task_output_stream` instead drains
/// the reader to `/dev/null` so the child process doesn't deadlock.
#[derive(Debug)]
pub enum OmniEventKind {
    // Execution lifecycle
    TaskStarted(TaskStartedEvent),
    TaskCompleted(TaskCompletedEvent),
    TaskFailed(TaskFailedEvent),
    TaskSkipped(TaskSkippedEvent),
    TaskRetrying(TaskRetryingEvent),
    CacheHit(CacheHitEvent),
    ExecutionComplete(ExecutionCompleteEvent),
    ExecutionPlanReady(ExecutionPlanReadyEvent),
    // Shared diagnostic
    Diagnostic(DiagnosticEvent),
    // Generator lifecycle
    GeneratorStarted(GeneratorStartEvent),
    GeneratorActionSkipped(GeneratorActionSkippedEvent),
    GeneratorActionInProgress(GeneratorActionInProgressEvent),
    GeneratorActionSuccess(GeneratorActionSuccessEvent),
    GeneratorActionFailed(GeneratorActionFailedEvent),
    GeneratorFileCreated(GeneratorFileCreatedEvent),
    GeneratorFileSkipped(GeneratorFileSkippedEvent),
    GeneratorComplete(GeneratorCompletedEvent),
}

/// A subscriber that sends every event into an unbounded channel.
/// Useful for asserting event sequences in tests.
///
/// `TaskOutputStreamEvent` is NOT forwarded; the reader is drained to
/// `/dev/null` to avoid deadlocking the child process.
pub struct ChannelSubscriber {
    tx: UnboundedSender<OmniEventKind>,
}

impl ChannelSubscriber {
    /// Creates a new `(ChannelSubscriber, receiver)` pair.
    pub fn new() -> (Self, UnboundedReceiver<OmniEventKind>) {
        let (tx, rx) = unbounded_channel();
        (Self { tx }, rx)
    }

    fn send(&self, event: OmniEventKind) {
        // Channel is unbounded; ignore send errors (receiver dropped).
        let _ = self.tx.send(event);
    }
}

impl DiagnosticSubscriber for ChannelSubscriber {
    async fn on_diagnostic(&self, e: DiagnosticEvent) {
        self.send(OmniEventKind::Diagnostic(e));
    }
}

impl ExecutionEventSubscriber for ChannelSubscriber {
    fn wants_task_output_stream(&self) -> bool {
        // We still want task output stream so we can drain it (avoid deadlock),
        // but we don't forward bytes to the channel.
        true
    }

    async fn on_task_output_stream(&self, mut event: TaskOutputStreamEvent) {
        // Drain to /dev/null. Tests that need raw bytes should write a custom
        // subscriber instead.
        tokio::spawn(async move {
            let _ = tokio::io::copy(
                &mut event.stream.reader,
                &mut tokio::io::sink(),
            )
            .await;
        });
    }

    async fn on_task_started(&self, e: TaskStartedEvent) {
        self.send(OmniEventKind::TaskStarted(e));
    }
    async fn on_task_completed(&self, e: TaskCompletedEvent) {
        self.send(OmniEventKind::TaskCompleted(e));
    }
    async fn on_task_failed(&self, e: TaskFailedEvent) {
        self.send(OmniEventKind::TaskFailed(e));
    }
    async fn on_task_skipped(&self, e: TaskSkippedEvent) {
        self.send(OmniEventKind::TaskSkipped(e));
    }
    async fn on_task_retrying(&self, e: TaskRetryingEvent) {
        self.send(OmniEventKind::TaskRetrying(e));
    }
    async fn on_cache_hit(&self, e: CacheHitEvent) {
        self.send(OmniEventKind::CacheHit(e));
    }
    async fn on_execution_complete(&self, e: ExecutionCompleteEvent) {
        self.send(OmniEventKind::ExecutionComplete(e));
    }
    async fn on_execution_plan_ready(&self, e: ExecutionPlanReadyEvent) {
        self.send(OmniEventKind::ExecutionPlanReady(e));
    }
}

impl GeneratorEventSubscriber for ChannelSubscriber {
    async fn on_generator_start(&self, e: GeneratorStartEvent) {
        self.send(OmniEventKind::GeneratorStarted(e));
    }
    async fn on_file_created(&self, e: GeneratorFileCreatedEvent) {
        self.send(OmniEventKind::GeneratorFileCreated(e));
    }
    async fn on_file_skipped(&self, e: GeneratorFileSkippedEvent) {
        self.send(OmniEventKind::GeneratorFileSkipped(e));
    }
    async fn on_generator_completed(&self, e: GeneratorCompletedEvent) {
        self.send(OmniEventKind::GeneratorComplete(e));
    }
    async fn on_action_skipped(&self, e: GeneratorActionSkippedEvent) {
        self.send(OmniEventKind::GeneratorActionSkipped(e));
    }
    async fn on_action_in_progress(&self, e: GeneratorActionInProgressEvent) {
        self.send(OmniEventKind::GeneratorActionInProgress(e));
    }
    async fn on_action_failed(&self, e: GeneratorActionFailedEvent) {
        self.send(OmniEventKind::GeneratorActionFailed(e));
    }
    async fn on_action_success(&self, e: GeneratorActionSuccessEvent) {
        self.send(OmniEventKind::GeneratorActionSuccess(e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskStartedEvent;

    #[tokio::test]
    async fn channel_subscriber_collects_events() {
        let (sub, mut rx) = ChannelSubscriber::new();

        sub.on_task_started(TaskStartedEvent {
            task_id: "p::t".to_string(),
            project: "p".to_string(),
            task: "t".to_string(),
        })
        .await;

        let event = rx.try_recv().expect("should have received an event");
        assert!(matches!(event, OmniEventKind::TaskStarted(_)));
    }
}
