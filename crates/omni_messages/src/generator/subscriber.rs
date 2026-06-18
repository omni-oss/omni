use crate::diagnostic::DiagnosticSubscriber;

use super::events::{
    GeneratorCompletedEvent, GeneratorFileCreatedEvent,
    GeneratorFileSkippedEvent, GeneratorStartEvent,
};

/// Subscriber for generator lifecycle events.
///
/// Requires [`DiagnosticSubscriber`] — implement that trait on your type to
/// receive (or opt out of) diagnostic events.
///
/// All methods are `async` (AFIT — no `#[async_trait]` required for generic
/// usage). All methods take `&self`.
pub trait GeneratorEventSubscriber: DiagnosticSubscriber {
    async fn on_generator_start(&self, _event: GeneratorStartEvent) {}
    async fn on_file_created(&self, _event: GeneratorFileCreatedEvent) {}
    async fn on_file_skipped(&self, _event: GeneratorFileSkippedEvent) {}
    async fn on_generator_completed(&self, _event: GeneratorCompletedEvent) {}
}

/// Forward all subscriber calls through a shared reference.
/// `DiagnosticSubscriber for &S` is covered by the blanket in `diagnostic.rs`.
impl<S: GeneratorEventSubscriber> GeneratorEventSubscriber for &S {
    async fn on_generator_start(&self, event: GeneratorStartEvent) {
        S::on_generator_start(*self, event).await
    }
    async fn on_file_created(&self, event: GeneratorFileCreatedEvent) {
        S::on_file_created(*self, event).await
    }
    async fn on_file_skipped(&self, event: GeneratorFileSkippedEvent) {
        S::on_file_skipped(*self, event).await
    }
    async fn on_generator_completed(&self, event: GeneratorCompletedEvent) {
        S::on_generator_completed(*self, event).await
    }
}
