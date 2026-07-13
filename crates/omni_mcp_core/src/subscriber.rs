use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use omni_messages::{
    DiagnosticSubscriber, ExecutionEventSubscriber, GeneratorEventSubscriber,
    TaskCompletedEvent, TaskFailedEvent, TaskOutputStreamEvent,
};
use omni_task_output_logs::LogsDisplay;
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
/// strings keyed by full task name (`"project#task"`).
pub struct McpSubscriber {
    /// Global output-log policy used to decide `wants_task_output_stream`.
    include_logs: LogsDisplay,
    /// In-flight per-task captures (task_id → handle + facet).
    captures: Arc<Mutex<HashMap<String, PendingCapture>>>,
    /// Resolved logs per task (task_id → UTF-8 text).
    logs: Arc<Mutex<HashMap<String, String>>>,
}

impl McpSubscriber {
    pub fn new(include_logs: LogsDisplay) -> Self {
        Self {
            include_logs,
            captures: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Drain all captured logs, leaving the internal map empty.
    ///
    /// Call this after the API execution completes — at that point every task's
    /// terminal event has fired and all drain handles have been awaited, so the
    /// map is fully populated.
    pub fn take_logs(&self) -> HashMap<String, String> {
        std::mem::take(&mut *self.logs.lock().unwrap())
    }

    /// Await the in-flight drain for `task_id`, then store the text when the
    /// resolved facet says to show it for the given failure state.
    async fn finish_capture(&self, task_id: &str, failed: bool) {
        // Take the capture out of the map before awaiting — this releases the
        // lock so other tasks can proceed concurrently.
        let capture = self.captures.lock().unwrap().remove(task_id);

        if let Some(capture) = capture {
            let bytes = capture.handle.await.unwrap_or_default();
            if capture.facet.should_show(failed) && !bytes.is_empty() {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                self.logs.lock().unwrap().insert(task_id.to_string(), text);
            }
        }
    }
}

impl DiagnosticSubscriber for McpSubscriber {
    fn wants_diagnostics(&self) -> bool {
        false
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
                self.logs.lock().unwrap().insert(event.task_id, text);
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

        self.captures
            .lock()
            .unwrap()
            .insert(event.task_id, PendingCapture { handle, facet });
    }

    async fn on_task_completed(&self, event: TaskCompletedEvent) {
        let failed = event.exit_code != 0;
        self.finish_capture(&event.task_id, failed).await;
    }

    async fn on_task_failed(&self, event: TaskFailedEvent) {
        self.finish_capture(&event.task_id, true).await;
    }
}

impl GeneratorEventSubscriber for McpSubscriber {}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_messages::TaskOutputStream;
    use omni_task_output_logs::EffectiveOutputLogs;
    use std::time::Duration;

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
            project: "proj".to_string(),
            task: "task".to_string(),
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
            project: "proj".to_string(),
            task: "task".to_string(),
            exit_code,
            elapsed: Duration::ZERO,
            cache_hit: false,
            tries: 1,
        }
    }

    fn failed(task_id: &str) -> TaskFailedEvent {
        TaskFailedEvent {
            task_id: task_id.to_string(),
            project: "proj".to_string(),
            task: "task".to_string(),
            error: "boom".to_string(),
            tries: 1,
        }
    }

    fn log_of<'a>(
        logs: &'a HashMap<String, String>,
        task_id: &str,
    ) -> Option<&'a str> {
        logs.get(task_id).map(String::as_str)
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
        assert_eq!(log_of(&logs, "proj#task"), Some("OUT"));
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
        assert_eq!(log_of(&logs, "t"), Some("ERR"));
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
        assert_eq!(log_of(&logs, "t"), Some("ERR"));
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
        assert_eq!(log_of(&logs, "t"), Some("CACHED"));
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
    async fn logs_are_keyed_by_task_id() {
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
        assert_eq!(log_of(&logs, "a#build"), Some("AAA"));
        assert_eq!(log_of(&logs, "b#build"), Some("BBB"));
        assert_eq!(logs.len(), 2);
    }
}
