use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

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
use omni_term_ui::mux_output_presenter::{
    MuxOutputPresenter as _, MuxOutputPresenterExt as _,
    MuxOutputPresenterStatic,
};
use owo_colors::OwoColorize as _;
use parking_lot::Mutex;
use tiny_gradient::{Gradient, GradientStr as _};
use tokio::task::JoinSet;

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
    /// Probe whether `indicatif` can actually display on this terminal, and
    /// pick the matching backend. `ProgressDrawTarget::stderr()` yields a
    /// hidden target when stderr is not an interactive terminal, which is our
    /// signal to fall back to log-based progress.
    fn new() -> Self {
        if ProgressDrawTarget::stderr().is_hidden() {
            GeneratorProgress::Log
        } else {
            GeneratorProgress::Indicatif {
                multi: MultiProgress::new(),
                current: None,
            }
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

/// CLI-layer event subscriber — the only place in the codebase that uses
/// `owo_colors` and `tiny_gradient` for terminal output.
///
/// Wraps a [`MuxOutputPresenterStatic`] for raw byte-stream multiplexing
/// (task stdout/stderr routing) and renders all lifecycle events as colored
/// log messages.
pub struct CliSubscriber {
    /// Initialised either at construction time (Stream/Tui modes) or by
    /// `on_execution_plan_ready` (Auto mode). Falls back to stream if the
    /// event never fires.
    mux: Arc<OnceLock<MuxOutputPresenterStatic>>,
    mode: CliUiMode,
    tasks: Arc<Mutex<JoinSet<()>>>,
    /// Generator progress renderer (indicatif spinner or log fallback).
    ///
    /// Created lazily on the first generator event so the terminal probe and
    /// `MultiProgress` allocation are skipped entirely for non-generator runs.
    generator_progress: Arc<Mutex<Option<GeneratorProgress>>>,
}

impl CliSubscriber {
    /// Always use stream output mode.
    pub fn new_stream() -> Self {
        let mux = Arc::new(OnceLock::new());
        let _ = mux.set(MuxOutputPresenterStatic::new_stream());
        Self {
            mux,
            mode: CliUiMode::Stream,
            tasks: Arc::new(Mutex::new(JoinSet::new())),
            generator_progress: Arc::new(Mutex::new(None)),
        }
    }

    /// Always use TUI output mode (downgraded to stream when not on a TTY).
    pub fn new_tui() -> Self {
        let presenter = if atty::is(atty::Stream::Stdout) {
            MuxOutputPresenterStatic::new_tui()
        } else {
            MuxOutputPresenterStatic::new_stream()
        };
        let mux = Arc::new(OnceLock::new());
        let _ = mux.set(presenter);
        Self {
            mux,
            mode: CliUiMode::Tui,
            tasks: Arc::new(Mutex::new(JoinSet::new())),
            generator_progress: Arc::new(Mutex::new(None)),
        }
    }

    /// Defer the stream/TUI decision until the execution plan is ready.
    ///
    /// [`on_execution_plan_ready`] will choose TUI when the plan contains
    /// interactive or persistent tasks **and** stdout is a TTY; stream
    /// otherwise. This replicates the original `Ui::Auto` semantics.
    ///
    /// [`on_execution_plan_ready`]: ExecutionEventSubscriber::on_execution_plan_ready
    pub fn new_auto() -> Self {
        Self {
            mux: Arc::new(OnceLock::new()),
            mode: CliUiMode::Auto,
            tasks: Arc::new(Mutex::new(JoinSet::new())),
            generator_progress: Arc::new(Mutex::new(None)),
        }
    }

    /// Return the initialised presenter, falling back to stream if
    /// `on_execution_plan_ready` has not yet fired (should not happen in
    /// normal use, but guards against edge cases).
    fn get_mux(&self) -> &MuxOutputPresenterStatic {
        self.mux
            .get_or_init(|| MuxOutputPresenterStatic::new_stream())
    }

    /// Run `f` against the generator progress renderer, creating it on first
    /// use (this is where the terminal capability probe happens).
    fn with_progress(&self, f: impl FnOnce(&mut GeneratorProgress)) {
        let mut guard = self.generator_progress.lock();
        f(guard.get_or_insert_with(GeneratorProgress::new));
    }

    /// Wait for all task output streams to finish draining.
    pub async fn wait(&self) {
        let _ = self.get_mux().wait().await;
        let mut tasks = self.tasks.lock();
        while let Some(_) = tasks.join_next().await {}
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
    }

    async fn on_execution_plan_ready(&self, e: ExecutionPlanReadyEvent) {
        if self.mode == CliUiMode::Auto {
            let presenter = if e.has_interactive_or_persistent_tasks
                && atty::is(atty::Stream::Stdout)
            {
                MuxOutputPresenterStatic::new_tui()
            } else {
                MuxOutputPresenterStatic::new_stream()
            };
            // set() is a no-op if already initialised — safe to call multiple times
            let _ = self.mux.set(presenter);
        }
    }

    async fn on_task_output_stream(&self, event: TaskOutputStreamEvent) {
        // Add the stream to the multiplexer. Use helpers (add_stream_output /
        // add_stream_full) which take generic R: MuxOutputPresenterReader so
        // the compiler can apply the blanket impl.
        let mux = Arc::clone(&self.mux);
        let id = event.task_id;
        let reader = event.stream.reader;
        let writer = event.stream.writer;

        self.tasks.lock().spawn(async move {
            // Initialise lazily inside the spawned task (should already be set).
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
    }

    async fn on_task_completed(&self, e: TaskCompletedEvent) {
        if e.cache_hit {
            // Cache hit message is already emitted in on_cache_hit.
            // Only log a failure if the cached result had a non-zero exit.
            if e.exit_code != 0 {
                log::error!(
                    "{}",
                    format!(
                        "Task '{}' failed with exit code {}",
                        e.task_id, e.exit_code
                    )
                    .red()
                );
            }
        } else if e.exit_code == 0 {
            log::info!("{}", format!("Executed task '{}'", e.task_id).green());
        } else {
            log::error!(
                "{}",
                format!(
                    "Task '{}' failed with exit code {}",
                    e.task_id, e.exit_code
                )
                .red()
            );
        }
    }

    async fn on_task_failed(&self, e: TaskFailedEvent) {
        log::error!(
            "{}",
            format!("Task '{}' error: {}", e.task_id, e.error).red()
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
        log::info!("{}", msg.white().dimmed());
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
        if e.has_logs {
            log::info!(
                "{} {}",
                format!("Cache hit for task '{}'", e.task_id).green(),
                if e.replay_logs {
                    "(replaying logs)".dimmed()
                } else {
                    "(skipping logs)".dimmed()
                }
            );
        } else {
            log::info!(
                "{}",
                format!("Cache hit for task '{}'", e.task_id).green(),
            );
        }
    }

    async fn on_execution_complete(&self, e: ExecutionCompleteEvent) {
        // Flush all task output streams before printing the summary.
        let _ = self.get_mux().wait().await;

        if e.succeeded > 0 {
            log::info!(
                "{}",
                format!(
                    "Successfully executed {} tasks ({} results from cache)",
                    e.succeeded, e.cache_hits,
                )
                .green()
                .bold()
            );
        }
        if e.failed > 0 {
            log::info!(
                "{}",
                format!("Failed to execute {} tasks", e.failed).red().bold()
            );
        }
        if e.skipped > 0 {
            log::info!(
                "{}",
                format!("Skipped {} tasks", e.skipped).yellow().bold()
            );
        }
        if e.cache_hits > 0 {
            log::info!(
                "{}",
                format!(
                    "Saved time in total from cached results ({:?})",
                    e.total_time_saved
                )
                .gradient(Gradient::Instagram)
                .bold()
            );
        }
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
