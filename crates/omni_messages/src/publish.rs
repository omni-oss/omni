use crate::notice::NoticeSubscriber;
pub use crate::notice::{NoticeEvent, NoticeLevel};

/// Publishes one event through `sub`: [`NoticeSubscriber::publish_notice`]
/// mirrors it to `tracing` and delivers it. This does not check
/// `wants_notices`.
///
/// Redaction belongs to the operation that produced the event, so pass an
/// [`OperationPublisher`](crate::OperationPublisher) when the event may carry
/// operation secrets. A bare subscriber delivers the event unchanged.
pub fn publish_event<S: NoticeSubscriber + ?Sized>(
    sub: &S,
    event: NoticeEvent,
) {
    sub.publish_notice(event);
}

/// Publishes a batch built without a subscriber (for example by a planner).
/// If the subscriber opts out of notices, the batch is not dispatched.
pub fn publish_batch<S: NoticeSubscriber + ?Sized>(
    sub: &S,
    events: impl IntoIterator<Item = NoticeEvent>,
) {
    if !sub.wants_notices() {
        return;
    }
    for e in events {
        publish_event(sub, e);
    }
}

/// Mirrors a notice into `tracing` at its own level, with its code. Every
/// published notice goes through here, so trace files stay complete.
#[cfg(feature = "tracing")]
pub fn mirror_to_tracing(e: &NoticeEvent) {
    let (code, msg, source) = (e.code(), e.message(), e.target());
    match e.level() {
        NoticeLevel::Info => tracing::info!(code, source, "{msg}"),
        NoticeLevel::Warn => tracing::warn!(code, source, "{msg}"),
        NoticeLevel::Error => tracing::error!(code, source, "{msg}"),
    }
}

#[cfg(not(feature = "tracing"))]
pub fn mirror_to_tracing(_: &NoticeEvent) {}

/// Emits a [`Notice`](omni_notice::Notice) through a subscriber.
///
/// The notice value is only constructed, and the event only built, when the
/// subscriber's `wants_notices()` returns `true`. `target` is the invoking
/// module's path.
///
/// ```rust,ignore
/// notice!(sub, SandboxOsDisabled);
/// notice!(sub, CacheWriteSkipped { entry: id.clone() });
/// ```
#[macro_export]
macro_rules! notice {
    ($subscriber:expr, $notice:expr $(,)?) => {
        if $crate::notice::NoticeSubscriber::wants_notices(&$subscriber) {
            $crate::publish::publish_event(
                &$subscriber,
                $crate::notice_event!($notice),
            )
        }
    };
}

/// Like [`notice!`], adding a diagnostic's `details` and `hints` to the
/// notice's `fields`. The code, level and message come from the notice.
#[macro_export]
macro_rules! diagnostic_notice {
    ($subscriber:expr, $notice:expr, $diagnostic:expr $(,)?) => {
        if $crate::notice::NoticeSubscriber::wants_notices(&$subscriber) {
            $crate::publish::publish_event(
                &$subscriber,
                $crate::NoticeEvent::from_diagnostic_notice(
                    &$notice,
                    $diagnostic,
                    ::std::module_path!(),
                ),
            )
        }
    };
}

/// Builds a [`NoticeEvent`] from a notice, for batch publishing.
#[macro_export]
macro_rules! notice_event {
    ($notice:expr $(,)?) => {
        $crate::NoticeEvent::from_notice(&$notice, ::std::module_path!())
    };
}

pub use {diagnostic_notice, notice, notice_event};

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use omni_diagnostic::Diagnostic;
    use omni_notice::Notice;

    use super::*;
    use crate::{
        GeneratorEventSubscriber, NoopSubscriber,
        execution::ExecutionEventSubscriber,
    };

    omni_codes::prefixes!("msgtest");

    /// Sample notice.
    #[derive(Notice)]
    #[notice(code = "msgtest.sample", level = warn,
             message = "hello {name} {secret}")]
    struct Sample {
        name: String,
        secret: String,
    }

    /// Sample essential.
    #[derive(Notice)]
    #[notice(code = "msgtest.essential", level = warn, essential,
             message = "static")]
    struct Essential;

    #[derive(Notice)]
    enum CacheNotice {
        /// Could not cache an entry.
        #[notice(code = "msgtest.cache_write", level = error,
                 message = "could not cache {entry}")]
        Write { entry: String },
        /// Cache is disabled.
        #[notice(code = "msgtest.cache_disabled", level = warn, essential,
                 message = "cache is disabled")]
        Disabled,
    }

    #[derive(Debug, thiserror::Error, Diagnostic)]
    #[error("boom")]
    #[diagnostic(code = "msgtest.fatal", hint = "try again")]
    struct Fatal;

    struct Collect {
        events: Arc<Mutex<Vec<NoticeEvent>>>,
    }

    impl NoticeSubscriber for Collect {
        fn on_notice(&self, e: NoticeEvent) {
            self.events.lock().unwrap().push(e);
        }
    }
    impl ExecutionEventSubscriber for Collect {}
    impl GeneratorEventSubscriber for Collect {}

    fn collect() -> (Collect, Arc<Mutex<Vec<NoticeEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        (
            Collect {
                events: events.clone(),
            },
            events,
        )
    }

    #[test]
    fn notice_dispatches_with_code_level_and_fields() {
        let (sub, events) = collect();
        notice!(
            sub,
            Sample {
                name: "a".into(),
                secret: "b".into()
            }
        );
        let e = events.lock().unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].code(), "msgtest.sample");
        assert_eq!(e[0].level(), NoticeLevel::Warn);
        assert!(!e[0].essential());
        assert_eq!(e[0].message(), "hello a b");
        assert_eq!(e[0].fields()["name"], "a");
        assert!(e[0].target().contains("publish"));
    }

    #[test]
    fn essential_flag_comes_from_the_notice() {
        let (sub, events) = collect();
        notice!(sub, Essential);
        assert!(events.lock().unwrap()[0].essential());
    }

    #[test]
    fn enum_notices_work_with_both_emission_macros() {
        let (sub, events) = collect();
        notice!(
            sub,
            CacheNotice::Write {
                entry: "task".into()
            }
        );
        diagnostic_notice!(sub, CacheNotice::Disabled, &Fatal);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].code(), "msgtest.cache_write");
        assert_eq!(events[0].level(), NoticeLevel::Error);
        assert_eq!(events[0].message(), "could not cache task");
        assert_eq!(events[0].fields()["entry"], "task");
        assert!(!events[0].fields().contains_key("variant"));
        assert_eq!(events[1].code(), "msgtest.cache_disabled");
        assert!(events[1].essential());
        assert_eq!(events[1].fields()["hints"][0], "try again");
    }

    #[test]
    fn notice_is_not_constructed_when_unwanted() {
        let sub = NoopSubscriber;
        assert!(!sub.wants_notices());
        let built = std::cell::Cell::new(false);
        let make = || {
            built.set(true);
            Essential
        };
        notice!(sub, make());
        assert!(!built.get(), "value must not be built for a disabled sink");
    }

    #[test]
    fn diagnostic_notice_adds_details_and_hints_only() {
        let (sub, events) = collect();
        diagnostic_notice!(sub, Essential, &Fatal);
        let e = events.lock().unwrap();
        assert_eq!(e[0].code(), "msgtest.essential");
        assert_eq!(e[0].message(), "static");
        assert_eq!(e[0].fields()["hints"][0], "try again");
        // The body is a CLI layout aid and never travels in `fields`.
        assert!(!e[0].fields().contains_key("body"));
    }

    #[test]
    fn secrets_are_redacted_in_message_and_fields() {
        let (inner, events) = collect();
        let sub = crate::OperationPublisher::new(
            &inner,
            omni_redact::Redactor::builder().value("hunter2").build(),
        );
        notice!(
            sub,
            Sample {
                name: "n".into(),
                secret: "hunter2".into()
            }
        );
        let e = events.lock().unwrap();
        assert!(!e[0].message().contains("hunter2"));
        assert!(
            !e[0].fields()["secret"]
                .as_str()
                .unwrap()
                .contains("hunter2")
        );
    }

    /// A tracing sink that records every event it sees, as text.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn a_secret_never_reaches_a_custom_tracing_sink() {
        let (inner, events) = collect();
        let sub = crate::OperationPublisher::new(
            &inner,
            omni_redact::Redactor::builder().value("hunter2").build(),
        );
        let sink = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            // Tests run in parallel, and another may have registered the
            // mirror's callsite while no subscriber existed, which caches
            // "never". A permissive global default keeps every callsite
            // interested, and the cache is rebuilt once it is installed.
            static GLOBAL: std::sync::Once = std::sync::Once::new();
            GLOBAL.call_once(|| {
                let _ = tracing::subscriber::set_global_default(
                    tracing_subscriber::fmt()
                        .with_writer(std::io::sink)
                        .with_max_level(tracing::Level::TRACE)
                        .finish(),
                );
            });
            tracing::callsite::rebuild_interest_cache();
            notice!(
                sub,
                Sample {
                    name: "n".into(),
                    secret: "hunter2".into()
                }
            );
        });

        let traced = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(traced.contains("msgtest.sample"), "mirrored: {traced}");
        assert!(!traced.contains("hunter2"), "leaked into trace: {traced}");
        assert!(!events.lock().unwrap()[0].message().contains("hunter2"));
    }

    #[test]
    fn batch_is_published_and_skipped_when_unwanted() {
        let (sub, events) = collect();
        publish_batch(
            &sub,
            [notice_event!(Essential), notice_event!(Essential)],
        );
        assert_eq!(events.lock().unwrap().len(), 2);
        publish_batch(&NoopSubscriber, [notice_event!(Essential)]);
    }

    #[test]
    fn wire_format_omits_target_and_uses_lowercase_level() {
        let e = notice_event!(Essential);
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["level"], "warn");
        assert_eq!(v["code"], "msgtest.essential");
        assert!(v.get("target").is_none());
    }
}
