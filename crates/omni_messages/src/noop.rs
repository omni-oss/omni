use crate::diagnostic::DiagnosticSubscriber;
use crate::execution::ExecutionEventSubscriber;
use crate::generator::GeneratorEventSubscriber;

/// Zero-sized subscriber that discards all events and creates no pipe
/// infrastructure. Use this in tests or contexts where events are irrelevant.
pub struct NoopSubscriber;

impl DiagnosticSubscriber for NoopSubscriber {
    fn wants_diagnostics(&self) -> bool {
        false
    }
}

impl ExecutionEventSubscriber for NoopSubscriber {
    fn wants_task_output_stream(&self) -> bool {
        false
    }
    fn wants_task_input_stream(&self) -> bool {
        false
    }
}

impl GeneratorEventSubscriber for NoopSubscriber {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_subscriber_is_zero_sized() {
        assert_eq!(std::mem::size_of::<NoopSubscriber>(), 0);
    }

    #[test]
    fn noop_does_not_want_streams() {
        assert!(!NoopSubscriber.wants_task_output_stream());
        assert!(!NoopSubscriber.wants_task_input_stream());
    }

    #[test]
    fn noop_does_not_want_diagnostics() {
        assert!(!NoopSubscriber.wants_diagnostics());
    }
}
