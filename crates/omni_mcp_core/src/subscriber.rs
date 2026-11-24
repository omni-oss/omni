use std::sync::{Arc, Mutex};

use omni_messages::{
    ExecutionEventSubscriber, GeneratorEventSubscriber, NoticeEvent,
    NoticeSubscriber, TaskCompletedEvent, TaskFailedEvent,
    TaskOutputStreamEvent, TaskStartedEvent,
};
use omni_task_output_logs::LogsDisplay;
use omni_types::TaskMap;

use crate::notices::{NoticeCollector, NoticePayload};
use tokio::io::AsyncReadExt as _;
use tokio::task::JoinHandle;

/// Per-task in-flight capture: a drain handle and the resolved display facet.
struct PendingCapture {
    handle: JoinHandle<Vec<u8>>,
    /// The resolved display facet for this task (from the stream event).
    facet: LogsDisplay,
}

/// An in-memory subscriber that captures task stdout/stderr for inclusion in
/// [`TaskExecutionSummary`](crate::model::TaskExecutionSummary).
///
/// Each task's combined output stream is drained into a `Vec<u8>`. For a fresh
/// (cache-miss) execution the bytes are stored only once the task's terminal
/// event (`on_task_completed` or `on_task_failed`) fires, and only when the
/// resolved display facet says to show them (e.g. the task failed and the
/// policy is `Failed`). Replayed cache-hit output has no terminal event and is
/// emitted only when the cached facet already permits it, so it is drained and
/// stored inline as it arrives.
///
/// After execution call [`McpSubscriber::take_logs`] to retrieve the captured
/// strings keyed by `(project, task)`.
pub struct McpSubscriber {
    /// Global output-log policy used to decide `wants_task_output_stream`.
    include_logs: LogsDisplay,
    /// In-flight per-task captures ((project, task) → handle + facet).
    captures: Arc<Mutex<TaskMap<PendingCapture>>>,
    /// Resolved logs per task ((project, task) → UTF-8 text).
    logs: Arc<Mutex<TaskMap<String>>>,
    /// Notices for this request: deduplicated and bounded.
    notices: Mutex<NoticeCollector>,
    /// Dry-run commands per task ((project, task) → redacted command line).
    commands: Mutex<TaskMap<String>>,
}

impl McpSubscriber {
    pub fn new(include_logs: LogsDisplay) -> Self {
        Self {
            include_logs,
            captures: Arc::new(Mutex::new(TaskMap::new())),
            logs: Arc::new(Mutex::new(TaskMap::new())),
            notices: Mutex::new(NoticeCollector::new()),
            commands: Mutex::new(TaskMap::new()),
        }
    }

    /// Takes the notices collected so far, appending the truncation entry
    /// when some were omitted. Call once, after the request has finished.
    pub fn take_notices(&self) -> Vec<NoticePayload> {
        std::mem::replace(
            &mut *self.notices.lock().unwrap(),
            NoticeCollector::new(),
        )
        .finish()
    }

    /// Dry-run command lines keyed by `(project, task)`.
    pub fn take_commands(&self) -> TaskMap<String> {
        std::mem::take(&mut *self.commands.lock().unwrap())
    }

    /// Drain all captured logs, leaving the internal map empty.
    ///
    /// Call this after the API execution completes — at that point every task's
    /// terminal event has fired and all drain handles have been awaited, so the
    /// map is fully populated.
    pub fn take_logs(&self) -> TaskMap<String> {
        std::mem::take(&mut *self.logs.lock().unwrap())
    }

    /// Await the in-flight drain for the task, then store the text when the
    /// resolved facet says to show it for the given failure state.
    async fn finish_capture(&self, project: &str, task: &str, failed: bool) {
        // Take the capture out of the map before awaiting — this releases the
        // lock so other tasks can proceed concurrently.
        let capture = self.captures.lock().unwrap().remove(project, task);

        if let Some(capture) = capture {
            let bytes = capture.handle.await.unwrap_or_default();
            if capture.facet.should_show(failed) && !bytes.is_empty() {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                self.logs.lock().unwrap().insert(project, task, text);
            }
        }
    }
}

impl NoticeSubscriber for McpSubscriber {
    fn on_notice(&self, event: NoticeEvent) {
        self.notices.lock().unwrap().push(event);
    }
}

impl ExecutionEventSubscriber for McpSubscriber {
    /// Only request streams when logs might be shown; `Never` means we never
    /// need the pipe infrastructure.
    fn wants_task_output_stream(&self) -> bool {
        self.include_logs != LogsDisplay::Never
    }

    async fn on_task_output_stream(&self, event: TaskOutputStreamEvent) {
        // Resolve the correct display facet for this specific task stream.
        let facet = if event.is_replay {
            event.output_logs.cached
        } else {
            event.output_logs.new
        };

        if facet == LogsDisplay::Never {
            // Drain the stream to prevent the child process from deadlocking,
            // but discard the bytes.
            let mut reader = event.stream.reader;
            tokio::spawn(async move {
                let _ =
                    tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
            });
            return;
        }

        // Replayed cache-hit output has no terminal task event
        // (`on_task_completed`/`on_task_failed`) to finalize a pending capture,
        // and the executor only emits this stream when the cached facet already
        // permits showing it. Drain the cached file to completion here and
        // store it directly rather than parking it in `captures`.
        if event.is_replay {
            let mut reader = event.stream.reader;
            let mut buf = Vec::new();
            let _ = reader.read_to_end(&mut buf).await;
            if !buf.is_empty() {
                let text = String::from_utf8_lossy(&buf).into_owned();
                self.logs.lock().unwrap().insert(
                    &event.project,
                    &event.task,
                    text,
                );
            }
            return;
        }

        // Drain the full stream to an in-memory buffer.
        let mut reader = event.stream.reader;
        let handle = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = reader.read_to_end(&mut buf).await;
            buf
        });

        self.captures.lock().unwrap().insert(
            &event.project,
            &event.task,
            PendingCapture { handle, facet },
        );
    }

    async fn on_task_started(&self, event: TaskStartedEvent) {
        if let Some(command) = event.command {
            self.commands.lock().unwrap().insert(
                &event.project,
                &event.task,
                command,
            );
        }
    }

    async fn on_task_completed(&self, event: TaskCompletedEvent) {
        let failed = event.exit_code != 0;
        self.finish_capture(&event.project, &event.task, failed)
            .await;
    }

    async fn on_task_failed(&self, event: TaskFailedEvent) {
        self.finish_capture(&event.project, &event.task, true).await;
    }
}

impl GeneratorEventSubscriber for McpSubscriber {}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_messages::TaskOutputStream;
    use omni_task_output_logs::EffectiveOutputLogs;
    use std::time::Duration;

    /// `project#task` ids name that pair; any other id is `proj`/`task`.
    fn pair(task_id: &str) -> (String, String) {
        match task_id.split_once('#') {
            Some((p, t)) => (p.to_string(), t.to_string()),
            None => ("proj".to_string(), "task".to_string()),
        }
    }

    fn facets(new: LogsDisplay, cached: LogsDisplay) -> EffectiveOutputLogs {
        EffectiveOutputLogs { new, cached }
    }

    fn stream_event(
        task_id: &str,
        is_replay: bool,
        output_logs: EffectiveOutputLogs,
        bytes: &str,
    ) -> TaskOutputStreamEvent {
        TaskOutputStreamEvent {
            task_id: task_id.to_string(),
            project: pair(task_id).0,
            task: pair(task_id).1,
            is_replay,
            is_interactive: false,
            output_logs,
            stream: TaskOutputStream {
                reader: Box::new(std::io::Cursor::new(
                    bytes.as_bytes().to_vec(),
                )),
                writer: None,
            },
        }
    }

    fn completed(task_id: &str, exit_code: u32) -> TaskCompletedEvent {
        TaskCompletedEvent {
            task_id: task_id.to_string(),
            project: pair(task_id).0,
            task: pair(task_id).1,
            exit_code,
            elapsed: Duration::ZERO,
            cache_hit: false,
            tries: 1,
        }
    }

    fn failed(task_id: &str) -> TaskFailedEvent {
        TaskFailedEvent {
            task_id: task_id.to_string(),
            project: pair(task_id).0,
            task: pair(task_id).1,
            error: "boom".to_string(),
            tries: 1,
        }
    }

    fn log_of(logs: &TaskMap<String>) -> Option<&str> {
        logs.get("proj", "task").map(String::as_str)
    }

    #[tokio::test]
    async fn one_subscriber_serves_operations_without_sharing_their_secrets() {
        let sub = McpSubscriber::new(LogsDisplay::Never);
        let start = |id: &str, command: &str| TaskStartedEvent {
            task_id: id.into(),
            project: "p".into(),
            task: id.trim_start_matches("p#").into(),
            command: Some(command.into()),
        };

        let a = omni_messages::RedactingSubscriber::new(
            &sub,
            omni_redact::Redactor::builder().value("alpha").build(),
        );
        let b = omni_messages::RedactingSubscriber::new(
            &sub,
            omni_redact::Redactor::builder().value("bravo").build(),
        );
        a.on_task_started(start("p#a", "run alpha bravo")).await;
        b.on_task_started(start("p#b", "run alpha bravo")).await;

        let commands = sub.take_commands();
        assert_eq!(commands.get("p", "a").unwrap(), "run <redacted> bravo");
        assert_eq!(commands.get("p", "b").unwrap(), "run alpha <redacted>");
    }

    #[test]
    fn notices_are_redacted_before_they_are_collected() {
        use omni_notice::Notice;

        omni_codes::prefixes!("mcpsub");

        /// Sample.
        #[derive(Notice)]
        #[notice(code = "mcpsub.sample", level = warn, message = "got {v}")]
        struct Sample {
            v: String,
        }

        let sub = McpSubscriber::new(LogsDisplay::Never);
        let op = omni_messages::RedactingSubscriber::new(
            &sub,
            omni_redact::Redactor::builder().value("hunter2").build(),
        );
        omni_messages::publish::publish_event(
            &op,
            omni_messages::NoticeEvent::from_notice(
                &Sample {
                    v: "hunter2".into(),
                },
                "t",
            ),
        );

        let notices = sub.take_notices();
        assert_eq!(notices.len(), 1);
        assert!(!notices[0].message.contains("hunter2"));
        assert!(!notices[0].fields["v"].as_str().unwrap().contains("hunter2"));
    }

    #[test]
    fn wants_stream_unless_never() {
        assert!(
            McpSubscriber::new(LogsDisplay::All).wants_task_output_stream()
        );
        assert!(
            McpSubscriber::new(LogsDisplay::Failed).wants_task_output_stream()
        );
        assert!(
            !McpSubscriber::new(LogsDisplay::Never).wants_task_output_stream()
        );
    }

    #[tokio::test]
    async fn fresh_output_stored_on_success_when_all() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        sub.on_task_output_stream(stream_event(
            "proj#task",
            false,
            facets(LogsDisplay::All, LogsDisplay::Never),
            "OUT",
        ))
        .await;
        sub.on_task_completed(completed("proj#task", 0)).await;

        let logs = sub.take_logs();
        assert_eq!(log_of(&logs), Some("OUT"));
    }

    #[tokio::test]
    async fn fresh_success_omitted_under_failed_policy() {
        let sub = McpSubscriber::new(LogsDisplay::Failed);
        sub.on_task_output_stream(stream_event(
            "t",
            false,
            facets(LogsDisplay::Failed, LogsDisplay::Failed),
            "OUT",
        ))
        .await;
        sub.on_task_completed(completed("t", 0)).await;

        assert!(sub.take_logs().is_empty());
    }

    #[tokio::test]
    async fn fresh_failure_shown_under_failed_policy() {
        let sub = McpSubscriber::new(LogsDisplay::Failed);
        sub.on_task_output_stream(stream_event(
            "t",
            false,
            facets(LogsDisplay::Failed, LogsDisplay::Failed),
            "ERR",
        ))
        .await;
        sub.on_task_completed(completed("t", 1)).await;

        let logs = sub.take_logs();
        assert_eq!(log_of(&logs), Some("ERR"));
    }

    #[tokio::test]
    async fn fresh_failure_via_on_task_failed_is_shown() {
        let sub = McpSubscriber::new(LogsDisplay::Failed);
        sub.on_task_output_stream(stream_event(
            "t",
            false,
            facets(LogsDisplay::Failed, LogsDisplay::Failed),
            "ERR",
        ))
        .await;
        sub.on_task_failed(failed("t")).await;

        let logs = sub.take_logs();
        assert_eq!(log_of(&logs), Some("ERR"));
    }

    // The fresh path consults the `new` facet, not `cached`: a `Never` new
    // facet discards the output even though `cached` would show it.
    #[tokio::test]
    async fn fresh_path_uses_new_facet() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        sub.on_task_output_stream(stream_event(
            "t",
            false,
            facets(LogsDisplay::Never, LogsDisplay::All),
            "OUT",
        ))
        .await;
        sub.on_task_completed(completed("t", 0)).await;

        assert!(sub.take_logs().is_empty());
    }

    #[tokio::test]
    async fn fresh_empty_output_not_stored() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        sub.on_task_output_stream(stream_event(
            "t",
            false,
            facets(LogsDisplay::All, LogsDisplay::All),
            "",
        ))
        .await;
        sub.on_task_completed(completed("t", 0)).await;

        assert!(sub.take_logs().is_empty());
    }

    // A cache hit emits the replay stream with no terminal event; the bytes
    // must still be stored inline. The `cached` facet is consulted (here it
    // shows while the `new` facet would not).
    #[tokio::test]
    async fn replay_output_stored_inline_without_terminal_event() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        sub.on_task_output_stream(stream_event(
            "t",
            true,
            facets(LogsDisplay::Never, LogsDisplay::All),
            "CACHED",
        ))
        .await;

        let logs = sub.take_logs();
        assert_eq!(log_of(&logs), Some("CACHED"));
    }

    #[tokio::test]
    async fn replay_never_facet_discards_output() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        sub.on_task_output_stream(stream_event(
            "t",
            true,
            facets(LogsDisplay::All, LogsDisplay::Never),
            "CACHED",
        ))
        .await;

        assert!(sub.take_logs().is_empty());
    }

    #[tokio::test]
    async fn replay_empty_output_not_stored() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        sub.on_task_output_stream(stream_event(
            "t",
            true,
            facets(LogsDisplay::All, LogsDisplay::All),
            "",
        ))
        .await;

        assert!(sub.take_logs().is_empty());
    }

    #[tokio::test]
    async fn take_logs_drains_the_map() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        sub.on_task_output_stream(stream_event(
            "t",
            true,
            facets(LogsDisplay::All, LogsDisplay::All),
            "X",
        ))
        .await;

        assert!(!sub.take_logs().is_empty());
        assert!(sub.take_logs().is_empty());
    }

    #[tokio::test]
    async fn logs_are_keyed_by_project_and_task() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        // A replayed task and a freshly executed one, interleaved.
        sub.on_task_output_stream(stream_event(
            "a#build",
            true,
            facets(LogsDisplay::All, LogsDisplay::All),
            "AAA",
        ))
        .await;
        sub.on_task_output_stream(stream_event(
            "b#build",
            false,
            facets(LogsDisplay::All, LogsDisplay::All),
            "BBB",
        ))
        .await;
        sub.on_task_completed(completed("b#build", 0)).await;

        let logs = sub.take_logs();
        assert_eq!(logs.get("a", "build").map(String::as_str), Some("AAA"));
        assert_eq!(logs.get("b", "build").map(String::as_str), Some("BBB"));
        assert_eq!(logs.len(), 2);
    }

    #[tokio::test]
    async fn formerly_colliding_pairs_keep_their_own_logs_and_commands() {
        let sub = McpSubscriber::new(LogsDisplay::All);
        for (project, task, bytes) in
            [("a#b", "c", "FIRST"), ("a", "b#c", "SECOND")]
        {
            let mut event = stream_event(
                r"x",
                false,
                facets(LogsDisplay::All, LogsDisplay::All),
                bytes,
            );
            event.project = project.into();
            event.task = task.into();
            sub.on_task_output_stream(event).await;
        }
        for (project, task, command) in
            [("a#b", "c", "cmd-first"), ("a", "b#c", "cmd-second")]
        {
            sub.on_task_started(TaskStartedEvent {
                task_id: "x".into(),
                project: project.into(),
                task: task.into(),
                command: Some(command.into()),
            })
            .await;
        }
        for (project, task) in [("a#b", "c"), ("a", "b#c")] {
            let mut done = completed("x", 0);
            done.project = project.into();
            done.task = task.into();
            sub.on_task_completed(done).await;
        }
        let logs = sub.take_logs();
        assert_eq!(logs.get("a#b", "c").map(String::as_str), Some("FIRST"));
        assert_eq!(logs.get("a", "b#c").map(String::as_str), Some("SECOND"));
        let commands = sub.take_commands();
        assert_eq!(
            commands.get("a#b", "c").map(String::as_str),
            Some("cmd-first")
        );
        assert_eq!(
            commands.get("a", "b#c").map(String::as_str),
            Some("cmd-second")
        );
        assert_eq!(commands.len(), 2);
    }
}
