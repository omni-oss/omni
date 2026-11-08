//! Operation-scoped publishing (RFC 0026).
//!
//! An [`RedactingSubscriber`] borrows a subscriber (the *sink*) and owns an
//! immutable [`Redactor`] for one operation: a task, a generator invocation,
//! or a tool call. Everything a producer emits through it is sanitized with
//! that redactor *before* it reaches the sink, the `tracing` mirror, or any
//! queue behind the sink. A sink therefore never holds an operation's
//! secrets, and reusing or sharing one sink across operations cannot carry
//! secrets from one to another.
//!
//! A [`child`](RedactingSubscriber::child) subscriber inherits a snapshot of its
//! parent and adds its own values. It never changes its parent or its
//! siblings. Both keep publishing to the same sink; the type does not nest.

use std::{borrow::Cow, sync::Arc};

use omni_redact::Redactor;

use crate::{
    execution::{
        CacheHitEvent, ExecutionCompleteEvent, ExecutionEventSubscriber,
        ExecutionPlanReadyEvent, TaskCompletedEvent, TaskFailedEvent,
        TaskOutputStreamEvent, TaskRetryingEvent, TaskSkippedEvent,
        TaskStartedEvent,
        events::{BatchCompletedEvent, BatchStartEvent},
    },
    generator::{
        GeneratorCompletedEvent, GeneratorEventSubscriber,
        GeneratorPromptRejectedEvent, GeneratorStartEvent, ScriptLogEvent,
        events::{
            GeneratorActionFailedEvent, GeneratorActionInProgressEvent,
            GeneratorActionSkippedEvent, GeneratorActionSuccessEvent,
        },
    },
    notice::{NoticeEvent, NoticeSubscriber},
};

/// Publishes one operation's output to a shared sink, sanitized with the
/// operation's own immutable redactor.
pub struct RedactingSubscriber<'a, S: ?Sized> {
    sink: &'a S,
    redactor: Arc<Redactor>,
}

impl<S: ?Sized> Clone for RedactingSubscriber<'_, S> {
    fn clone(&self) -> Self {
        Self {
            sink: self.sink,
            redactor: self.redactor.clone(),
        }
    }
}

impl<S: ?Sized> std::fmt::Debug for RedactingSubscriber<'_, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the redactor: it holds the operation's secrets.
        f.debug_struct("RedactingSubscriber")
            .finish_non_exhaustive()
    }
}

impl<'a, S: ?Sized> RedactingSubscriber<'a, S> {
    /// A redacting subscriber for an operation that owns `redactor`.
    pub fn new(sink: &'a S, redactor: impl Into<Arc<Redactor>>) -> Self {
        Self {
            sink,
            redactor: redactor.into(),
        }
    }

    /// A redacting subscriber with no secrets of its own, for a run root that has no
    /// values shared by everything under it.
    pub fn root(sink: &'a S) -> Self {
        Self::new(sink, Redactor::builder().build())
    }

    /// A redacting subscriber for an operation nested in this one. It hides everything
    /// this one hides plus `local`, and is unaffected by later children.
    pub fn child(&self, local: &Redactor) -> Self {
        let redactor = if local.is_empty() {
            self.redactor.clone()
        } else {
            Arc::new(self.redactor.union(local))
        };
        Self {
            sink: self.sink,
            redactor,
        }
    }

    /// Replaces this operation's secrets in `text`.
    pub fn redact<'t>(&self, text: impl Into<Cow<'t, str>>) -> Cow<'t, str> {
        self.redactor.redact(text)
    }

    /// The underlying subscriber, for building a nested run's configuration.
    /// Publishing through it skips this operation's sanitizing, so nothing but
    /// scope construction should use it.
    pub fn sink(&self) -> &'a S {
        self.sink
    }

    /// Replaces this operation's secrets in `text` in place.
    fn clean(&self, text: &mut String) {
        if let Cow::Owned(redacted) = self.redactor.redact(text.as_str()) {
            *text = redacted;
        }
    }

    /// Like [`clean`](Self::clean), for text that may be absent.
    fn clean_opt(&self, text: &mut Option<String>) {
        if let Some(t) = text {
            self.clean(t);
        }
    }

    /// Whether there is anything to hide, so empty scopes skip all work.
    fn enabled(&self) -> bool {
        !self.redactor.is_empty()
    }
}

impl<S: NoticeSubscriber + ?Sized> NoticeSubscriber
    for RedactingSubscriber<'_, S>
{
    fn wants_notices(&self) -> bool {
        self.sink.wants_notices()
    }

    fn publish_notice(&self, mut event: NoticeEvent) {
        event.redact(&self.redactor);
        self.sink.publish_notice(event);
    }

    fn on_notice(&self, mut event: NoticeEvent) {
        event.redact(&self.redactor);
        self.sink.on_notice(event);
    }
}

impl<S: ExecutionEventSubscriber + ?Sized> ExecutionEventSubscriber
    for RedactingSubscriber<'_, S>
{
    fn wants_task_output_stream(&self) -> bool {
        self.sink.wants_task_output_stream()
    }

    fn wants_task_input_stream(&self) -> bool {
        self.sink.wants_task_input_stream()
    }

    async fn on_task_started(&self, mut e: TaskStartedEvent) {
        if self.enabled() {
            self.clean_opt(&mut e.command);
        }
        self.sink.on_task_started(e).await
    }

    async fn on_execution_plan_ready(&self, e: ExecutionPlanReadyEvent) {
        self.sink.on_execution_plan_ready(e).await
    }

    /// Raw task output is passed through (RFC 0022 limitation).
    async fn on_task_output_stream(&self, e: TaskOutputStreamEvent) {
        self.sink.on_task_output_stream(e).await
    }

    async fn on_task_completed(&self, e: TaskCompletedEvent) {
        self.sink.on_task_completed(e).await
    }

    async fn on_task_failed(&self, mut e: TaskFailedEvent) {
        if self.enabled() {
            self.clean(&mut e.error);
        }
        self.sink.on_task_failed(e).await
    }

    async fn on_task_skipped(&self, mut e: TaskSkippedEvent) {
        // `dependency` is shown to users; `task_id` is routing identity and
        // stays as it is.
        if self.enabled() {
            self.clean_opt(&mut e.dependency);
        }
        self.sink.on_task_skipped(e).await
    }

    async fn on_task_retrying(&self, e: TaskRetryingEvent) {
        self.sink.on_task_retrying(e).await
    }

    async fn on_cache_hit(&self, e: CacheHitEvent) {
        self.sink.on_cache_hit(e).await
    }

    async fn on_execution_complete(&self, e: ExecutionCompleteEvent) {
        self.sink.on_execution_complete(e).await
    }

    async fn on_batch_start(&self, e: BatchStartEvent) {
        self.sink.on_batch_start(e).await
    }

    async fn on_batch_completed(&self, e: BatchCompletedEvent) {
        self.sink.on_batch_completed(e).await
    }
}

impl<S: GeneratorEventSubscriber + ?Sized> GeneratorEventSubscriber
    for RedactingSubscriber<'_, S>
{
    async fn on_prompt_rejected(&self, mut e: GeneratorPromptRejectedEvent) {
        if self.enabled() {
            self.clean(&mut e.message);
        }
        self.sink.on_prompt_rejected(e).await
    }

    async fn on_generator_start(&self, e: GeneratorStartEvent) {
        self.sink.on_generator_start(e).await
    }

    async fn on_action_skipped(&self, mut e: GeneratorActionSkippedEvent) {
        if self.enabled() {
            self.clean_opt(&mut e.reason);
        }
        self.sink.on_action_skipped(e).await
    }

    async fn on_action_in_progress(
        &self,
        mut e: GeneratorActionInProgressEvent,
    ) {
        if self.enabled() {
            self.clean(&mut e.message);
        }
        self.sink.on_action_in_progress(e).await
    }

    async fn on_action_success(&self, mut e: GeneratorActionSuccessEvent) {
        if self.enabled() {
            self.clean(&mut e.message);
        }
        self.sink.on_action_success(e).await
    }

    async fn on_action_failed(&self, mut e: GeneratorActionFailedEvent) {
        if self.enabled() {
            self.clean(&mut e.message);
        }
        self.sink.on_action_failed(e).await
    }

    async fn on_generator_completed(&self, e: GeneratorCompletedEvent) {
        self.sink.on_generator_completed(e).await
    }

    fn on_script_log(&self, mut e: ScriptLogEvent) {
        if self.enabled() {
            self.clean(&mut e.message);
            self.clean(&mut e.target);
        }
        self.sink.on_script_log(e)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use omni_notice::Notice;

    use super::*;
    use crate::{notice, publish::publish_batch};

    omni_codes::prefixes!("scopetest");

    /// Sample.
    #[derive(Notice)]
    #[notice(code = "scopetest.sample", level = warn, message = "got {v}")]
    struct Sample {
        v: String,
    }

    /// A sink that records the text of everything it is handed.
    #[derive(Default)]
    struct Sink {
        seen: Mutex<Vec<String>>,
    }

    impl Sink {
        fn seen(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl NoticeSubscriber for Sink {
        fn on_notice(&self, e: NoticeEvent) {
            self.seen.lock().unwrap().push(format!(
                "{} {}",
                e.message(),
                serde_json::to_string(e.fields()).unwrap()
            ));
        }
    }

    impl ExecutionEventSubscriber for Sink {
        async fn on_task_started(&self, e: TaskStartedEvent) {
            self.seen
                .lock()
                .unwrap()
                .push(e.command.unwrap_or_default());
        }
        async fn on_task_failed(&self, e: TaskFailedEvent) {
            self.seen.lock().unwrap().push(e.error);
        }
        async fn on_task_skipped(&self, e: TaskSkippedEvent) {
            self.seen
                .lock()
                .unwrap()
                .push(e.dependency.unwrap_or_default());
        }
    }

    impl GeneratorEventSubscriber for Sink {
        async fn on_prompt_rejected(&self, e: GeneratorPromptRejectedEvent) {
            self.seen.lock().unwrap().push(e.message);
        }
        async fn on_action_failed(&self, e: GeneratorActionFailedEvent) {
            self.seen.lock().unwrap().push(e.message);
        }
        fn on_script_log(&self, e: ScriptLogEvent) {
            self.seen
                .lock()
                .unwrap()
                .push(format!("{} {}", e.target, e.message));
        }
    }

    fn secret(v: &str) -> Redactor {
        Redactor::builder().value(v).build()
    }

    fn leak(v: &str) -> Sample {
        Sample { v: v.into() }
    }

    #[test]
    fn concurrent_operations_do_not_mask_each_others_secrets() {
        let sink = Sink::default();
        let a = RedactingSubscriber::new(&sink, secret("alpha"));
        let b = RedactingSubscriber::new(&sink, secret("bravo"));

        // Interleaved, and each operation emits the other's literal too.
        notice!(a, leak("alpha bravo"));
        notice!(b, leak("alpha bravo"));
        notice!(a, leak("bravo"));

        let seen = sink.seen();
        assert!(seen[0].contains("<redacted> bravo"), "{}", seen[0]);
        assert!(seen[1].contains("alpha <redacted>"), "{}", seen[1]);
        assert!(seen[2].contains("bravo"), "{}", seen[2]);
        assert!(!seen[2].contains("<redacted>"), "{}", seen[2]);
    }

    #[test]
    fn a_child_inherits_the_parent_and_neither_leaks_upward_or_sideways() {
        let sink = Sink::default();
        let parent = RedactingSubscriber::new(&sink, secret("parent-secret"));
        let one = parent.child(&secret("child-one"));
        let two = parent.child(&secret("child-two"));

        notice!(one, leak("parent-secret child-one child-two"));
        notice!(two, leak("parent-secret child-one child-two"));
        notice!(parent, leak("parent-secret child-one child-two"));

        let seen = sink.seen();
        assert!(seen[0].contains("<redacted> <redacted> child-two"));
        assert!(seen[1].contains("<redacted> child-one <redacted>"));
        assert!(
            seen[2].contains("<redacted> child-one child-two"),
            "the parent must not learn its children's secrets: {}",
            seen[2]
        );
    }

    #[test]
    fn a_reused_sink_keeps_nothing_from_a_finished_operation() {
        let sink = Sink::default();
        {
            let first = RedactingSubscriber::new(&sink, secret("old-secret"));
            notice!(first, leak("old-secret"));
        }
        let second = RedactingSubscriber::new(&sink, secret("new-secret"));
        notice!(second, leak("old-secret new-secret"));

        let seen = sink.seen();
        assert!(seen[1].contains("old-secret <redacted>"), "{}", seen[1]);
    }

    #[test]
    fn the_sink_is_never_handed_a_redactor() {
        // The sink type has no redactor access by construction; the operation's
        // redactor is dropped with the subscriber.
        let sink = Sink::default();
        let redactor = Arc::new(secret("hunter2"));
        let weak = Arc::downgrade(&redactor);
        let redacting = RedactingSubscriber::new(&sink, redactor);
        let child = redacting.child(&secret("other"));
        notice!(child, leak("hunter2"));
        drop(redacting);
        drop(child);
        assert!(weak.upgrade().is_none(), "scopes must release the redactor");
    }

    #[test]
    fn batches_are_sanitized_by_the_operation() {
        let sink = Sink::default();
        let op = RedactingSubscriber::new(&sink, secret("hunter2"));
        publish_batch(
            &op,
            [crate::NoticeEvent::from_notice(&leak("hunter2"), "t")],
        );
        assert!(!sink.seen()[0].contains("hunter2"));
    }

    #[tokio::test]
    async fn success_and_failure_paths_are_sanitized() {
        let sink = Sink::default();
        let op = RedactingSubscriber::new(&sink, secret("hunter2"));

        op.on_task_started(TaskStartedEvent {
            task_id: "p#t".into(),
            project: "p".into(),
            task: "t".into(),
            command: Some("deploy --token hunter2".into()),
        })
        .await;
        op.on_task_failed(TaskFailedEvent {
            task_id: "p#t".into(),
            project: "p".into(),
            task: "t".into(),
            error: "failed with hunter2".into(),
            tries: 1,
        })
        .await;
        op.on_task_skipped(TaskSkippedEvent {
            task_id: "p#t".into(),
            project: "p".into(),
            task: "t".into(),
            reason: crate::TaskSkipReason::DependeeTaskFailure,
            dependency: Some("p#hunter2".into()),
        })
        .await;
        op.on_prompt_rejected(GeneratorPromptRejectedEvent {
            message: "bad value hunter2".into(),
        })
        .await;
        op.on_action_failed(GeneratorActionFailedEvent {
            name: "a".into(),
            message: "boom hunter2".into(),
            depth: 0,
        })
        .await;
        op.on_script_log(ScriptLogEvent {
            level: crate::ScriptLogLevel::Info,
            target: "hunter2".into(),
            message: "hunter2".into(),
        });

        let seen = sink.seen();
        assert_eq!(seen.len(), 6);
        for line in seen {
            assert!(!line.contains("hunter2"), "leaked: {line}");
            assert!(line.contains("<redacted>"), "{line}");
        }
    }

    #[test]
    fn secrets_never_reach_the_tracing_mirror() {
        use std::io::Write;

        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);
        impl Write for Capture {
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

        let sink = Sink::default();
        let op = RedactingSubscriber::new(&sink, secret("hunter2"));
        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();

        tracing::subscriber::with_default(subscriber, || {
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
            notice!(op, leak("hunter2"));
        });

        let traced =
            String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(traced.contains("scopetest.sample"), "mirrored: {traced}");
        assert!(!traced.contains("hunter2"), "leaked into trace: {traced}");
    }

    #[test]
    fn a_secret_overlapping_the_placeholder_is_sanitized_once() {
        // A second pass over already-sanitized text would re-match the
        // placeholder text, so one publish must sanitize exactly once.
        let sink = Sink::default();
        let op = RedactingSubscriber::new(&sink, secret("redacted"));
        notice!(op, leak("redacted"));
        assert!(
            sink.seen()[0].starts_with("got <redacted> "),
            "{}",
            sink.seen()[0]
        );
        assert!(!sink.seen()[0].contains("<<"), "{}", sink.seen()[0]);
    }

    #[test]
    fn debug_output_does_not_contain_secrets() {
        let sink = Sink::default();
        let op = RedactingSubscriber::new(&sink, secret("hunter2"));
        assert!(!format!("{op:?}").contains("hunter2"));
    }
}
