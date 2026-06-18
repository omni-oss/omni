use tokio::io::{AsyncRead, AsyncWrite};

/// Owned byte streams for a spawned task's stdout/stderr (and optionally stdin).
/// The subscriber takes ownership and MUST fully consume `reader` (e.g., by
/// spawning a tokio task). Failure to drain causes the child process to deadlock.
pub struct TaskOutputStream {
    /// Combined stdout+stderr byte stream from the child process.
    pub reader: Box<dyn AsyncRead + Unpin + Send + Sync + 'static>,
    /// Present only when `ExecutionEventSubscriber::wants_task_input_stream() == true`
    /// AND the task is interactive/persistent.
    pub writer: Option<Box<dyn AsyncWrite + Unpin + Send + Sync + 'static>>,
}

/// Delivered to the subscriber once per task, before `on_task_completed`.
/// NOT `Clone` or `Serialize` — it carries live byte streams.
pub struct TaskOutputStreamEvent {
    /// Full task identifier, e.g. `"my_project::build"`.
    pub task_id: String,
    pub project: String,
    pub task: String,
    /// `true` when replaying cached logs, not live output.
    pub is_replay: bool,
    pub stream: TaskOutputStream,
}
