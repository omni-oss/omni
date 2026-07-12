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
/// Each task's combined output stream is drained into a `Vec<u8>`. Once the
/// task's terminal event (`on_task_completed` or `on_task_failed`) fires, the
/// captured bytes are converted to UTF-8 and stored under the task's full name
/// — but only when the resolved display facet says to show them (e.g. the task
/// failed and the policy is `Failed`).
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
