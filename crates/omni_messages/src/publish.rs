/// Conditionally emit a [`NoticeEvent`] through a subscriber.
///
/// The event is only constructed and dispatched when the subscriber's
/// `wants_notices()` returns `true`, avoiding unnecessary string
/// allocations for subscribers that discard notices (e.g. [`NoopSubscriber`]).
///
/// Only user-facing levels (`Info`, `Warn`, `Error`) are accepted; use
/// `log::debug!` / `log::trace!` for developer diagnostics.
///
/// The macro calls the subscriber's synchronous `on_notice` and returns `()`.
///
/// `target` is automatically set to the invoking module's path via
/// [`std::module_path!`].
///
/// # Examples
///
/// ```rust,ignore
/// use omni_messages::publish::{notice, NoticeLevel};
///
/// // Simple message
/// notice!(subscriber, NoticeLevel::Info, "task started");
///
/// // Format string
/// notice!(subscriber, NoticeLevel::Warn, "processed {}/{}", done, total);
/// ```
///
/// [`NoticeEvent`]: crate::NoticeEvent
/// [`NoopSubscriber`]: crate::NoopSubscriber
#[macro_export]
macro_rules! notice {
    ($subscriber:expr, $level:expr, $($fmt_args:tt)+) => {
        if $subscriber.wants_notices() {
            $subscriber
                .on_notice(
                    $crate::notice_event!($level, $($fmt_args)+)
                )
        }
    };
}

#[macro_export]
macro_rules! notice_event {
    ($level:expr, $($fmt_args:tt)+) => {
        $crate::NoticeEvent {
            level: $level,
            message: ::std::format!($($fmt_args)+),
            fields: ::std::collections::BTreeMap::new(),
            target: ::std::module_path!().to_string(),
        }
    };
}

pub use notice;

// Re-export the types callers need alongside the macro.
pub use crate::notice::{NoticeEvent, NoticeLevel};

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::{
        GeneratorEventSubscriber, NoopSubscriber,
        execution::ExecutionEventSubscriber, notice::NoticeSubscriber,
    };

    /// A subscriber that counts how many `on_notice` calls it receives.
    struct CountingSubscriber(Arc<AtomicU32>);

    impl NoticeSubscriber for CountingSubscriber {
        fn on_notice(&self, _event: NoticeEvent) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl ExecutionEventSubscriber for CountingSubscriber {}
    impl GeneratorEventSubscriber for CountingSubscriber {}

    #[tokio::test]
    async fn notice_dispatches_when_wants_notices_is_true() {
        let count = Arc::new(AtomicU32::new(0));
        let sub = CountingSubscriber(Arc::clone(&count));
        assert!(sub.wants_notices(), "should want notices by default");

        notice!(sub, NoticeLevel::Info, "hello {}", 42);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn notice_skips_when_wants_notices_is_false() {
        let sub = NoopSubscriber;
        assert!(
            !sub.wants_notices(),
            "NoopSubscriber should not want notices"
        );

        // on_notice is never called; no panic expected
        notice!(sub, NoticeLevel::Error, "should not reach subscriber");
    }
}
