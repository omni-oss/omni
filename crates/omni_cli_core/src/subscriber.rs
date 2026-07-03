use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use comfy_table::{
    Cell, Color, Table, modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL,
};
use indicatif::{
    MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle,
};
use omni_messages::execution::events::BatchCompletedEvent;
use omni_messages::generator::events::{
    GeneratorActionFailedEvent, GeneratorActionInProgressEvent,
    GeneratorActionSkippedEvent, GeneratorActionSuccessEvent,
};
use omni_messages::{
    CacheHitEvent, DiagnosticEvent, DiagnosticLevel, DiagnosticSubscriber,
    ExecutionCompleteEvent, ExecutionEventSubscriber, ExecutionPlanReadyEvent,
    GeneratorCompletedEvent, GeneratorEventSubscriber, GeneratorStartEvent,
    TaskCompletedEvent, TaskFailedEvent, TaskOutputStreamEvent,
    TaskRetryingEvent, TaskSkipReason, TaskSkippedEvent, TaskStartedEvent,
};
use omni_task_output_logs::LogsDisplay;
use omni_term_ui::mux_output_presenter::{
    MuxOutputPresenter as _, MuxOutputPresenterExt as _,
    MuxOutputPresenterStatic,
};
use owo_colors::OwoColorize as _;
use parking_lot::Mutex;
use tiny_gradient::{Gradient, GradientStr as _};
use tokio::io::AsyncWriteExt as _;
use tokio::task::JoinSet;

use crate::task_output_capture::{self, CaptureTarget};

/// Which mode will be chosen when `on_execution_plan_ready` fires.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CliUiMode {
    /// Always use the stream presenter.
    Stream,
    /// Always use the TUI presenter (downgraded to stream when not on a TTY).
    Tui,
    /// Decide at plan-ready time: TUI when interactive/persistent tasks are
    /// present and stdout is a TTY; stream otherwise.
    Auto,
    /// Render a live progress bar with per-task spinners, capturing task output
    /// to disk. Downgrades to `Tui` when interactive/persistent tasks are
    /// present and to `Stream` off a TTY.
    Progress,
}

/// Interval between spinner frames when indicatif rendering is active.
const SPINNER_TICK: Duration = Duration::from_millis(80);

/// Renders generator progress, transparently choosing between a live
/// `indicatif` spinner and a plain log-based fallback.
///
/// The decision is made once, up front: if `indicatif` cannot draw to the
/// terminal (e.g. output is piped/redirected, or the terminal is "dumb"), the
/// draw target reports itself as hidden and we permanently fall back to
/// ordinary `log` lines. Otherwise we render an animated spinner per action.
///
/// Only **top-level** generator actions (nesting `depth == 0`) drive the
/// progress display. Actions belonging to sub-generators launched via
/// `run-generator` are subsumed under their parent's progress step, so their
/// events are ignored here.
enum GeneratorProgress {
    /// `indicatif` can render; `current` is the spinner for the in-flight
    /// top-level action, if any.
    Indicatif {
        multi: MultiProgress,
        current: Option<ProgressBar>,
    },
    /// Terminal cannot render `indicatif`; emit plain `log` lines instead.
    Log,
}

impl GeneratorProgress {
    /// Build the renderer from a shared, pre-probed `MultiProgress`. When
    /// `multi` is `None` the terminal cannot render `indicatif`, so we fall
    /// back to plain `log` lines.
    fn new(multi: Option<MultiProgress>) -> Self {
        match multi {
            Some(multi) => GeneratorProgress::Indicatif {
                multi,
                current: None,
            },
            None => GeneratorProgress::Log,
        }
    }

    fn spinner_style() -> ProgressStyle {
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
    }

    /// Clear the in-flight spinner (if any) without printing a final line.
    fn clear_current(current: &mut Option<ProgressBar>) {
        if let Some(pb) = current.take() {
            pb.finish_and_clear();
        }
    }

    fn generator_start(&mut self, name: &str) {
        match self {
            GeneratorProgress::Indicatif { multi, .. } => {
                let _ = multi.println(
                    format!("Running generator '{}'", name).bold().to_string(),
                );
            }
            GeneratorProgress::Log => {
                log::info!("Running generator '{}'", name);
            }
        }
    }

    fn action_in_progress(&mut self, name: &str, message: &str) {
        let text = format!("{}: {}", name, message);
        match self {
            GeneratorProgress::Indicatif { multi, current } => {
                // Guard against a dangling spinner (should not normally happen
                // as success/failed always finishes the previous action).
                Self::clear_current(current);
                let pb = multi.add(ProgressBar::new_spinner());
                pb.set_style(Self::spinner_style());
                pb.enable_steady_tick(SPINNER_TICK);
                pb.set_message(text);
                *current = Some(pb);
            }
            GeneratorProgress::Log => {
                log::info!("{}", text);
            }
        }
    }

    fn action_success(&mut self, name: &str, message: &str) {
        let text = format!("{}: {}", name, message);
        match self {
            GeneratorProgress::Indicatif { multi, current } => {
                Self::clear_current(current);
                let _ = multi.println(format!(
                    "{} {}",
                    "\u{2713}".green(),
                    text.green()
                ));
            }
            GeneratorProgress::Log => {
                log::info!("{}", text.green());
            }
        }
    }

    fn action_failed(&mut self, name: &str, message: &str) {
        let text = format!("{}: {}", name, message);
        match self {
            GeneratorProgress::Indicatif { multi, current } => {
                Self::clear_current(current);
                let _ = multi.println(format!(
                    "{} {}",
                    "\u{2717}".red(),
                    text.red()
                ));
            }
            GeneratorProgress::Log => {
                log::error!("{}", text.red());
            }
        }
    }

    fn action_skipped(&mut self, name: &str, reason: Option<&str>) {
        let msg = if let Some(reason) = reason
            && !reason.is_empty()
        {
            format!("Skipped action '{}' ({})", name, reason)
        } else {
            format!("Skipped action '{}'", name)
        };
        match self {
            GeneratorProgress::Indicatif { multi, .. } => {
                let _ = multi.println(
                    format!("{} {}", "\u{229d}", msg).dimmed().to_string(),
                );
            }
            GeneratorProgress::Log => {
                log::debug!("{}", msg);
            }
        }
    }

    fn generator_completed(&mut self, name: &str) {
        let msg = format!("Generator '{}' complete", name);
        match self {
            GeneratorProgress::Indicatif { multi, current } => {
                Self::clear_current(current);
                let _ = multi.println(msg.green().bold().to_string());
            }
            GeneratorProgress::Log => {
                log::info!("{}", msg.green().bold());
            }
        }
    }
}

/// Renders task-execution progress in `progress` mode: an aggregate
/// `[done/total]` bar plus a spinner per running task, mirroring
/// [`GeneratorProgress`]. Shares the subscriber's single `MultiProgress` so the
/// bar and any captured-log replays never fight for the terminal.
enum TaskProgress {
    Indicatif {
        multi: MultiProgress,
        overall: ProgressBar,
        running: HashMap<String, ProgressBar>,
    },
    Log,
}

impl TaskProgress {
    fn new(multi: Option<MultiProgress>, total: usize) -> Self {
        match multi {
            Some(multi) => {
                let overall = multi.add(ProgressBar::new(total as u64));
                overall.set_style(Self::overall_style());
                overall.enable_steady_tick(SPINNER_TICK);
                TaskProgress::Indicatif {
                    multi,
                    overall,
                    running: HashMap::new(),
                }
            }
            None => TaskProgress::Log,
        }
    }

    fn overall_style() -> ProgressStyle {
        ProgressStyle::with_template(
            "{spinner:.green} [{pos}/{len}] {wide_msg}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
    }

    fn task_started(&mut self, id: &str) {
        if let TaskProgress::Indicatif { multi, running, .. } = self {
            let pb = multi.add(ProgressBar::new_spinner());
            pb.set_style(GeneratorProgress::spinner_style());
            pb.enable_steady_tick(SPINNER_TICK);
            pb.set_message(id.to_string());
            running.insert(id.to_string(), pb);
        }
    }

    fn task_finished(&mut self, id: &str) {
        if let TaskProgress::Indicatif {
            overall, running, ..
        } = self
        {
            if let Some(pb) = running.remove(id) {
                pb.finish_and_clear();
            }
            overall.inc(1);
        }
    }

    /// Write captured bytes to stdout without corrupting the bar. In the
    /// `indicatif` backend the write happens inside `multi.suspend` so the bar
    /// is temporarily cleared; the `Log` backend writes directly.
    fn replay_bytes(&self, bytes: &[u8]) {
        match self {
            TaskProgress::Indicatif { multi, .. } => {
                multi.suspend(|| write_stdout(bytes));
            }
            TaskProgress::Log => write_stdout(bytes),
        }
    }

    fn println(&self, msg: &str) {
        match self {
            TaskProgress::Indicatif { multi, .. } => {
                let _ = multi.println(msg);
            }
            TaskProgress::Log => log::info!("{}", msg),
        }
    }

    fn finish(&mut self) {
        if let TaskProgress::Indicatif {
            overall, running, ..
        } = self
        {
            for (_, pb) in running.drain() {
                pb.finish_and_clear();
            }
            overall.finish_and_clear();
        }
    }
}

fn write_stdout(bytes: &[u8]) {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

/// A task's in-flight output capture: the drain task writing the reader to a
/// file (or sink) and the resolved display facet to apply once the task's
/// terminal event arrives.
struct CapturedOutput {
    drain: tokio::task::JoinHandle<std::io::Result<CaptureTarget>>,
    facet: LogsDisplay,
}

/// CLI-layer event subscriber — the only place in the codebase that uses
/// `owo_colors` and `tiny_gradient` for terminal output.
///
/// Wraps a [`MuxOutputPresenterStatic`] for raw byte-stream multiplexing
/// (task stdout/stderr routing) and renders all lifecycle events as colored
/// log messages.
pub struct CliSubscriber {
    /// Initialised either at construction time (Stream/Tui modes) or by
    /// `on_execution_plan_ready` (Auto/Progress modes). Falls back to stream if
    /// the event never fires.
    mux: Arc<OnceLock<MuxOutputPresenterStatic>>,
    /// The requested mode. `Auto`/`Progress` may resolve to a different concrete
    /// mode at plan-ready time.
    mode: CliUiMode,
    /// The concrete mode chosen at `on_execution_plan_ready`.
    resolved_mode: Arc<OnceLock<CliUiMode>>,
    /// Directory under which per-task capture files are written.
    scratch_dir: PathBuf,
    tasks: Arc<Mutex<JoinSet<()>>>,
    /// In-flight per-task output captures (drain handle + display facet).
    captures: Arc<Mutex<HashMap<String, CapturedOutput>>>,
    /// The single, lazily-probed `MultiProgress` shared by the generator and
    /// task progress renderers (`None` when the terminal cannot render).
    multi: Arc<OnceLock<Option<MultiProgress>>>,
    /// Generator progress renderer (indicatif spinner or log fallback).
    generator_progress: Arc<Mutex<Option<GeneratorProgress>>>,
    /// Task progress renderer, seeded in `progress` mode at plan-ready.
    task_progress: Arc<Mutex<Option<TaskProgress>>>,
}

impl CliSubscriber {
    fn new_with(
        mode: CliUiMode,
        scratch_dir: PathBuf,
        mux: Arc<OnceLock<MuxOutputPresenterStatic>>,
    ) -> Self {
        Self {
            mux,
            mode,
            resolved_mode: Arc::new(OnceLock::new()),
            scratch_dir,
            tasks: Arc::new(Mutex::new(JoinSet::new())),
            captures: Arc::new(Mutex::new(HashMap::new())),
            multi: Arc::new(OnceLock::new()),
            generator_progress: Arc::new(Mutex::new(None)),
            task_progress: Arc::new(Mutex::new(None)),
        }
    }

    /// Always use stream output mode.
    pub fn new_stream(scratch_dir: PathBuf) -> Self {
        let mux = Arc::new(OnceLock::new());
        let _ = mux.set(MuxOutputPresenterStatic::new_stream());
        Self::new_with(CliUiMode::Stream, scratch_dir, mux)
    }

    /// Always use TUI output mode (downgraded to stream when not on a TTY).
    pub fn new_tui(scratch_dir: PathBuf) -> Self {
        let presenter = if atty::is(atty::Stream::Stdout) {
            MuxOutputPresenterStatic::new_tui()
        } else {
            MuxOutputPresenterStatic::new_stream()
        };
        let mux = Arc::new(OnceLock::new());
        let _ = mux.set(presenter);
        Self::new_with(CliUiMode::Tui, scratch_dir, mux)
    }

    /// Defer the stream/TUI/progress decision until the execution plan is ready.
    ///
    /// [`on_execution_plan_ready`] chooses `stream` off a TTY, `tui` when the
    /// plan contains interactive/persistent tasks, and `progress` otherwise.
    ///
    /// [`on_execution_plan_ready`]: ExecutionEventSubscriber::on_execution_plan_ready
    pub fn new_auto(scratch_dir: PathBuf) -> Self {
        Self::new_with(CliUiMode::Auto, scratch_dir, Arc::new(OnceLock::new()))
    }

    /// Render a live progress bar, downgrading to `tui` for
    /// interactive/persistent plans and to `stream` off a TTY.
    pub fn new_progress(scratch_dir: PathBuf) -> Self {
        Self::new_with(
            CliUiMode::Progress,
            scratch_dir,
            Arc::new(OnceLock::new()),
        )
    }

    /// Return the initialised presenter, falling back to stream if
    /// `on_execution_plan_ready` has not yet fired (should not happen in
    /// normal use, but guards against edge cases).
    fn get_mux(&self) -> &MuxOutputPresenterStatic {
        self.mux
            .get_or_init(|| MuxOutputPresenterStatic::new_stream())
    }

    /// The concrete mode chosen at plan-ready, falling back to the requested
    /// mode if the plan-ready event has not fired.
    fn resolved_mode(&self) -> CliUiMode {
        self.resolved_mode.get().copied().unwrap_or(self.mode)
    }

    /// Lazily probe the terminal and return the shared `MultiProgress`
    /// (`None` when `indicatif` cannot render).
    fn shared_multi(&self) -> Option<MultiProgress> {
        self.multi
            .get_or_init(|| {
                if ProgressDrawTarget::stderr().is_hidden() {
                    None
                } else {
                    Some(MultiProgress::new())
                }
            })
            .clone()
    }

    /// Run `f` against the generator progress renderer, creating it on first
    /// use from the shared `MultiProgress`.
    fn with_progress(&self, f: impl FnOnce(&mut GeneratorProgress)) {
        let multi = self.shared_multi();
        let mut guard = self.generator_progress.lock();
        f(guard.get_or_insert_with(move || GeneratorProgress::new(multi)));
    }

    /// Run `f` against the task progress renderer, if it has been seeded.
    fn with_task_progress(&self, f: impl FnOnce(&mut TaskProgress)) {
        let mut guard = self.task_progress.lock();
        if let Some(tp) = guard.as_mut() {
            f(tp);
        }
    }

    /// Emit a lifecycle line, routing through the progress bar in `progress`
    /// mode so the bar is not corrupted, and through `log` otherwise.
    fn emit(&self, msg: String, is_error: bool) {
        if self.resolved_mode() == CliUiMode::Progress {
            self.with_task_progress(|tp| tp.println(&msg));
        } else if is_error {
            log::error!("{}", msg);
        } else {
            log::info!("{}", msg);
        }
    }

    /// Await a captured task's drain, replay its output if the resolved facet
    /// says to, then delete the capture file and advance the progress bar.
    async fn finish_capture(&self, task_id: &str, failed: bool) {
        let captured = self.captures.lock().remove(task_id);

        if let Some(captured) = captured {
            let target = captured.drain.await.ok().and_then(|r| r.ok());
            if let Some(CaptureTarget::File(path)) = target {
                if task_output_capture::should_display(captured.facet, failed) {
                    self.replay_file(&path).await;
                }
                let _ = tokio::fs::remove_file(&path).await;
            }
        }

        self.with_task_progress(|tp| tp.task_finished(task_id));
    }

    /// Replay a capture file to stdout, using `multi.suspend` in `progress`
    /// mode and a direct copy otherwise.
    async fn replay_file(&self, path: &Path) {
        if self.resolved_mode() == CliUiMode::Progress {
            if let Ok(bytes) = tokio::fs::read(path).await {
                self.with_task_progress(|tp| tp.replay_bytes(&bytes));
            }
        } else if let Ok(mut file) = tokio::fs::File::open(path).await {
            let mut stdout = tokio::io::stdout();
            let _ = tokio::io::copy(&mut file, &mut stdout).await;
            let _ = stdout.flush().await;
        }
    }

    /// Wait for all task output streams to finish draining.
    pub async fn wait(&self) {
        let _ = self.get_mux().wait().await;

        let remaining: Vec<_> =
            self.captures.lock().drain().map(|(_, c)| c.drain).collect();
        for handle in remaining {
            let _ = handle.await;
        }

        let mut tasks = self.tasks.lock();
        while tasks.join_next().await.is_some() {}
    }
}

impl DiagnosticSubscriber for CliSubscriber {
    async fn on_diagnostic(&self, e: DiagnosticEvent) {
        match e.level {
            DiagnosticLevel::Trace => log::trace!("{}", e.message),
            DiagnosticLevel::Debug => log::debug!("{}", e.message),
            DiagnosticLevel::Info => log::info!("{}", e.message),
            DiagnosticLevel::Warn => log::warn!("{}", e.message),
            DiagnosticLevel::Error => log::error!("{}", e.message),
        }
    }
}

impl ExecutionEventSubscriber for CliSubscriber {
    fn wants_task_output_stream(&self) -> bool {
        true
    }

    fn wants_task_input_stream(&self) -> bool {
        self.get_mux().accepts_input()
    }

    async fn on_task_started(&self, e: TaskStartedEvent) {
        log::debug!("Starting task '{}'", e.task_id);
        self.with_task_progress(|tp| tp.task_started(&e.task_id));
    }

    async fn on_execution_plan_ready(&self, e: ExecutionPlanReadyEvent) {
        // Best-effort sweep of any capture files leaked by a previously crashed
        // run, before this run starts writing its own. Runs before any capture
        // is created, so it never races this run's output.
        let _ = tokio::fs::remove_dir_all(self.scratch_dir.join("logs")).await;

        let is_tty = atty::is(atty::Stream::Stdout);

        // Resolve Auto/Progress to a concrete mode. Auto and Progress share the
        // same three-way heuristic; explicit Stream/Tui are honoured as-is.
        let resolved = match self.mode {
            CliUiMode::Auto | CliUiMode::Progress => {
                if !is_tty {
                    CliUiMode::Stream
                } else if e.has_interactive_or_persistent_tasks {
                    CliUiMode::Tui
                } else {
                    CliUiMode::Progress
                }
            }
            CliUiMode::Stream => CliUiMode::Stream,
            CliUiMode::Tui => CliUiMode::Tui,
        };
        let _ = self.resolved_mode.set(resolved);

        // Initialise the multiplexer for the resolved mode (no-op if already
        // set at construction for explicit Stream/Tui).
        let presenter = if resolved == CliUiMode::Tui {
            MuxOutputPresenterStatic::new_tui()
        } else {
            MuxOutputPresenterStatic::new_stream()
        };
        let _ = self.mux.set(presenter);

        // Seed the aggregate progress bar in progress mode.
        if resolved == CliUiMode::Progress {
            let multi = self.shared_multi();
            *self.task_progress.lock() =
                Some(TaskProgress::new(multi, e.total));
        }
    }

    async fn on_task_output_stream(&self, event: TaskOutputStreamEvent) {
        let resolved = self.resolved_mode();

        // A stream is passed through live (via the multiplexer) when it must
        // stay interactive, when the TUI owns rendering, or when stream mode is
        // asked to show all fresh output (or replay cached output). Everything
        // else is captured to disk and surfaced later per the display policy.
        let live_via_mux = event.is_interactive
            || resolved == CliUiMode::Tui
            || (resolved == CliUiMode::Stream
                && (event.is_replay
                    || event.output_logs.new == LogsDisplay::All));

        if live_via_mux {
            let mux = Arc::clone(&self.mux);
            let id = event.task_id;
            let reader = event.stream.reader;
            let writer = event.stream.writer;

            self.tasks.lock().spawn(async move {
                let presenter =
                    mux.get_or_init(|| MuxOutputPresenterStatic::new_stream());
                let handle_result = if let Some(w) = writer {
                    presenter.add_stream_full(id, reader, w).await
                } else {
                    presenter.add_stream_output(id, reader).await
                };
                if let Ok(handle) = handle_result {
                    let _ = handle.await;
                }
            });
            return;
        }

        // Replayed cached output reaches this branch only in `progress` mode
        // (stream/tui replay is live via the mux above). Cache hits emit no
        // terminal event, so there is nothing to defer to: drain the finite
        // replay stream and surface it through the bar immediately.
        if event.is_replay {
            let reader = event.stream.reader;
            let task_progress = Arc::clone(&self.task_progress);
            self.tasks.lock().spawn(async move {
                let mut reader = reader;
                let mut buf = Vec::new();
                let _ =
                    tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buf)
                        .await;
                if let Some(tp) = task_progress.lock().as_ref() {
                    tp.replay_bytes(&buf);
                }
            });
            return;
        }

        // Fresh-output capture path. `Never` drains to a sink; anything else
        // drains to a file that may be replayed once the task's terminal event
        // arrives.
        let facet = event.output_logs.new;
        let to_file = if facet == LogsDisplay::Never {
            None
        } else {
            Some(task_output_capture::capture_path(
                &self.scratch_dir,
                &event.project,
                &event.task,
            ))
        };

        let reader = event.stream.reader;
        let drain = tokio::spawn(async move {
            task_output_capture::drain(reader, to_file).await
        });

        self.captures
            .lock()
            .insert(event.task_id, CapturedOutput { drain, facet });
    }

    async fn on_task_completed(&self, e: TaskCompletedEvent) {
        let failed = e.exit_code != 0;

        // Drain and (conditionally) replay this task's captured output before
        // printing its status line.
        self.finish_capture(&e.task_id, failed).await;

        if e.cache_hit {
            // Cache hit message is already emitted in on_cache_hit.
            // Only report a failure if the cached result had a non-zero exit.
            if failed {
                self.emit(
                    format!(
                        "Task '{}' failed with exit code {}",
                        e.task_id, e.exit_code
                    )
                    .red()
                    .to_string(),
                    true,
                );
            }
        } else if !failed {
            self.emit(
                format!("Executed task '{}'", e.task_id).green().to_string(),
                false,
            );
        } else {
            self.emit(
                format!(
                    "Task '{}' failed with exit code {}",
                    e.task_id, e.exit_code
                )
                .red()
                .to_string(),
                true,
            );
        }
    }

    async fn on_task_failed(&self, e: TaskFailedEvent) {
        self.finish_capture(&e.task_id, true).await;
        self.emit(
            format!("Task '{}' error: {}", e.task_id, e.error)
                .red()
                .to_string(),
            true,
        );
    }

    async fn on_task_skipped(&self, e: TaskSkippedEvent) {
        let msg = match e.reason {
            TaskSkipReason::Disabled => {
                format!("Skipping disabled task '{}'", e.task_id)
            }
            TaskSkipReason::NoCommand => {
                format!("Skipping task '{}': no command to execute", e.task_id)
            }
            TaskSkipReason::PreviousBatchFailure => {
                format!(
                    "Skipping task '{}': a previous batch failed",
                    e.task_id
                )
            }
            TaskSkipReason::DependeeTaskFailure => {
                if let Some(dep) = &e.dependency {
                    format!(
                        "Skipping task '{}' due to failed dependency '{}'",
                        e.task_id, dep
                    )
                } else {
                    format!(
                        "Skipping task '{}': a dependee task failed",
                        e.task_id
                    )
                }
            }
        };
        self.emit(msg.white().dimmed().to_string(), false);
        self.with_task_progress(|tp| tp.task_finished(&e.task_id));
    }

    async fn on_task_retrying(&self, e: TaskRetryingEvent) {
        log::warn!(
            "Task '{}' is retrying... (attempt {}/{})",
            e.task_id,
            e.attempt,
            e.max_retries
        );
    }

    async fn on_cache_hit(&self, e: CacheHitEvent) {
        let msg = if e.has_logs {
            format!(
                "{} {}",
                format!("Cache hit for task '{}'", e.task_id).green(),
                if e.replay_logs {
                    "(replaying logs)".dimmed()
                } else {
                    "(skipping logs)".dimmed()
                }
            )
        } else {
            format!("Cache hit for task '{}'", e.task_id)
                .green()
                .to_string()
        };
        self.emit(msg, false);
        // Cache hits emit no terminal event, so advance the aggregate bar here.
        self.with_task_progress(|tp| tp.task_finished(&e.task_id));
    }

    async fn on_execution_complete(&self, e: ExecutionCompleteEvent) {
        // Flush all task output streams before printing the summary.
        let _ = self.get_mux().wait().await;

        if e.total > 0 {
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_header(vec!["Result", "Count"]);

            table.add_row(vec![Cell::new("Total"), Cell::new(e.total)]);
            table.add_row(vec![
                Cell::new("Succeeded").fg(Color::Green),
                Cell::new(e.succeeded).fg(Color::Green),
            ]);
            if e.failed > 0 {
                table.add_row(vec![
                    Cell::new("Failed").fg(Color::Red),
                    Cell::new(e.failed).fg(Color::Red),
                ]);
            }
            if e.skipped > 0 {
                table.add_row(vec![
                    Cell::new("Skipped").fg(Color::Yellow),
                    Cell::new(e.skipped).fg(Color::Yellow),
                ]);
            }
            if e.cache_hits > 0 {
                table.add_row(vec![
                    Cell::new("Cache hits").fg(Color::Cyan),
                    Cell::new(e.cache_hits).fg(Color::Cyan),
                ]);
            }
            table.add_row(vec![
                Cell::new("Elapsed").fg(Color::Blue),
                Cell::new(format!("{:?}", e.elapsed)).fg(Color::Blue),
            ]);
            if e.cache_hits > 0 {
                table.add_row(vec![
                    Cell::new(
                        "Time saved (cache)".gradient(Gradient::Instagram),
                    ),
                    Cell::new(
                        format!("{:?}", e.total_time_saved)
                            .gradient(Gradient::Instagram),
                    ),
                ]);
            }

            // Render the table cleanly: in progress mode through the bar's
            // `println` (so it does not corrupt the spinner), otherwise via a
            // direct `println!`. We deliberately bypass the line-oriented log
            // formatter, which would prefix and mangle the box-drawing borders,
            // but keep the same log-level gate (`log::log_enabled!` reads the
            // same max level as `log::info!`/`error!`) so `-l off` stays quiet.
            let rendered = table.to_string();
            if self.resolved_mode() == CliUiMode::Progress {
                self.with_task_progress(|tp| tp.println(&rendered));
            } else {
                let level = if e.failed > 0 {
                    log::Level::Error
                } else {
                    log::Level::Info
                };
                if log::log_enabled!(level) {
                    println!("{rendered}");
                }
            }
        }

        // Tear down the aggregate bar once the summary has been printed.
        self.with_task_progress(|tp| tp.finish());
    }

    async fn on_batch_completed(&self, _event: BatchCompletedEvent) {
        self.wait().await;
    }
}

impl GeneratorEventSubscriber for CliSubscriber {
    async fn on_generator_start(&self, e: GeneratorStartEvent) {
        self.with_progress(|p| p.generator_start(&e.name));
    }

    async fn on_action_in_progress(&self, e: GeneratorActionInProgressEvent) {
        // Only the top-level generator drives the progress display; actions
        // inside sub-generators are represented by their parent's step.
        if e.depth == 0 {
            self.with_progress(|p| p.action_in_progress(&e.name, &e.message));
        }
    }

    async fn on_action_success(&self, e: GeneratorActionSuccessEvent) {
        if e.depth == 0 {
            self.with_progress(|p| p.action_success(&e.name, &e.message));
        }
    }

    async fn on_action_failed(&self, e: GeneratorActionFailedEvent) {
        if e.depth == 0 {
            self.with_progress(|p| p.action_failed(&e.name, &e.message));
        }
    }

    async fn on_action_skipped(&self, e: GeneratorActionSkippedEvent) {
        if e.depth == 0 {
            self.with_progress(|p| {
                p.action_skipped(&e.name, e.reason.as_deref())
            });
        }
    }

    async fn on_generator_completed(&self, e: GeneratorCompletedEvent) {
        self.with_progress(|p| p.generator_completed(&e.name));
    }
}
