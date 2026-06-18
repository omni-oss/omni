/// Conditionally emit a [`DiagnosticEvent`] through a subscriber.
///
/// The event is only constructed and dispatched when the subscriber's
/// `wants_diagnostics()` returns `true`, avoiding unnecessary string
/// allocations for subscribers that discard diagnostics (e.g. [`NoopSubscriber`]).
///
/// The macro expands to a `Future` — callers must `.await` the result.
///
/// `target` is automatically set to the invoking module's path via
/// [`std::module_path!`].
///
/// # Examples
///
/// ```rust,ignore
/// use omni_messages::publish::{diagnostic, DiagnosticLevel};
///
/// // Simple message
/// diagnostic!(subscriber, DiagnosticLevel::Info, "task started").await;
///
/// // Format string
/// diagnostic!(subscriber, DiagnosticLevel::Debug, "processed {}/{}", done, total).await;
/// ```
///
/// [`DiagnosticEvent`]: crate::DiagnosticEvent
/// [`NoopSubscriber`]: crate::NoopSubscriber
#[macro_export]
macro_rules! diagnostic {
    ($subscriber:expr, $level:expr, $($fmt_args:tt)+) => {
        async {
            if $subscriber.wants_diagnostics() {
                $subscriber
                    .on_diagnostic($crate::DiagnosticEvent {
                        level: $level,
                        message: ::std::format!($($fmt_args)+),
                        fields: ::std::collections::BTreeMap::new(),
                        target: ::std::module_path!().to_string(),
                    })
                    .await
            }
        }
    };
}

pub use diagnostic;

// Re-export the types callers need alongside the macro.
pub use crate::diagnostic::{DiagnosticEvent, DiagnosticLevel};

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::{
        GeneratorEventSubscriber, NoopSubscriber,
        diagnostic::DiagnosticSubscriber, execution::ExecutionEventSubscriber,
    };

    /// A subscriber that counts how many `on_diagnostic` calls it receives.
    struct CountingSubscriber(Arc<AtomicU32>);

    impl DiagnosticSubscriber for CountingSubscriber {
        async fn on_diagnostic(&self, _event: DiagnosticEvent) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ExecutionEventSubscriber for CountingSubscriber {}
    impl GeneratorEventSubscriber for CountingSubscriber {}

    #[tokio::test]
    async fn diagnostic_dispatches_when_wants_diagnostics_is_true() {
        let count = Arc::new(AtomicU32::new(0));
        let sub = CountingSubscriber(Arc::clone(&count));
        assert!(
            sub.wants_diagnostics(),
            "should want diagnostics by default"
        );

        diagnostic!(sub, DiagnosticLevel::Info, "hello {}", 42).await;
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn diagnostic_skips_when_wants_diagnostics_is_false() {
        let sub = NoopSubscriber;
        assert!(
            !sub.wants_diagnostics(),
            "NoopSubscriber should not want diagnostics"
        );

        // on_diagnostic is never called; no panic expected
        diagnostic!(sub, DiagnosticLevel::Error, "should not reach subscriber")
            .await;
    }
}
