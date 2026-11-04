use crate::{
    generator::events::{
        GeneratorActionFailedEvent, GeneratorActionInProgressEvent,
        GeneratorActionSkippedEvent, GeneratorActionSuccessEvent,
    },
    notice::NoticeSubscriber,
};

use super::events::{GeneratorCompletedEvent, GeneratorStartEvent};
use super::script_log::ScriptLogEvent;

/// Subscriber for generator lifecycle events.
///
/// Requires [`NoticeSubscriber`] — implement that trait on your type to
/// receive (or opt out of) notice events.
///
/// All methods are `async` (AFIT — no `#[async_trait]` required for generic
/// usage). All methods take `&self`.
pub trait GeneratorEventSubscriber: NoticeSubscriber {
    async fn on_generator_start(&self, _event: GeneratorStartEvent) {}
    async fn on_action_skipped(&self, _event: GeneratorActionSkippedEvent) {}
    async fn on_action_in_progress(
        &self,
        _event: GeneratorActionInProgressEvent,
    ) {
    }
    async fn on_action_success(&self, _event: GeneratorActionSuccessEvent) {}
    async fn on_action_failed(&self, _event: GeneratorActionFailedEvent) {}
    async fn on_generator_completed(&self, _event: GeneratorCompletedEvent) {}

    /// A line a script logged. Synchronous so it can be delivered from the
    /// batch the script runner returns. The default sends it to `tracing`
    /// only; presenters override it to show `info`/`warn`/`error`.
    fn on_script_log(&self, event: ScriptLogEvent) {
        event.mirror_to_tracing();
    }
}

/// Forward all subscriber calls through a shared reference.
/// `NoticeSubscriber for &S` is covered by the blanket in `notice.rs`.
impl<S: GeneratorEventSubscriber> GeneratorEventSubscriber for &S {
    async fn on_generator_start(&self, event: GeneratorStartEvent) {
        S::on_generator_start(*self, event).await
    }
    async fn on_action_in_progress(
        &self,
        event: GeneratorActionInProgressEvent,
    ) {
        S::on_action_in_progress(*self, event).await
    }
    async fn on_action_skipped(&self, event: GeneratorActionSkippedEvent) {
        S::on_action_skipped(*self, event).await
    }
    async fn on_action_success(&self, event: GeneratorActionSuccessEvent) {
        S::on_action_success(*self, event).await
    }
    async fn on_action_failed(&self, event: GeneratorActionFailedEvent) {
        S::on_action_failed(*self, event).await
    }
    async fn on_generator_completed(&self, event: GeneratorCompletedEvent) {
        S::on_generator_completed(*self, event).await
    }
    fn on_script_log(&self, event: ScriptLogEvent) {
        S::on_script_log(*self, event)
    }
}
