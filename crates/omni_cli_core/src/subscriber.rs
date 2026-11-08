use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use comfy_table::TableStyle;
use comfy_table::{Cell, Color, Table, presets::UTF8_FULL};
use indicatif::{
    MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle,
};
use omni_command_config::escape_control_chars;
use omni_messages::execution::events::BatchCompletedEvent;
use omni_messages::generator::events::{
    GeneratorActionFailedEvent, GeneratorActionInProgressEvent,
    GeneratorActionSkippedEvent, GeneratorActionSuccessEvent,
};
use omni_messages::{
    CacheHitEvent, ExecutionCompleteEvent, ExecutionEventSubscriber,
    ExecutionPlanReadyEvent, GeneratorCompletedEvent, GeneratorEventSubscriber,
    GeneratorStartEvent, NoticeEvent, NoticeLevel, NoticeSubscriber,
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
    /// Terminal cannot render `indicatif` (or `-q` asked for no spinner):
    /// write plain status lines to stderr. Under `-q` only failures print.
    Log { quiet: bool },
}

impl GeneratorProgress {
    /// Build the renderer from a shared, pre-probed `MultiProgress`. When
    /// `multi` is `None` the terminal cannot render `indicatif`, so we fall
    /// back to plain `log` lines.
    fn new(multi: Option<MultiProgress>, quiet: bool) -> Self {
        match multi {
            Some(multi) => GeneratorProgress::Indicatif {
                multi,
                current: None,
            },
            None => GeneratorProgress::Log { quiet },
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
            GeneratorProgress::Log { quiet } => {
                if !*quiet {
                    eprintln!("Running generator '{}'", name);
                }
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
            GeneratorProgress::Log { quiet } => {
                if !*quiet {
                    eprintln!("{}", text);
                }
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
            GeneratorProgress::Log { quiet } => {
                if !*quiet {
                    eprintln!("{}", text.green());
                }
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
            GeneratorProgress::Log { .. } => {
                eprintln!("{}", text.red());
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
            GeneratorProgress::Log { .. } => {
                tracing::debug!("{}", msg);
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
            GeneratorProgress::Log { quiet } => {
                if !*quiet {
                    eprintln!("{}", msg.green().bold());
                }
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

    fn task_retrying(&self, id: &str, attempt: usize, max_retries: usize) {
        if let TaskProgress::Indicatif { running, .. } = self
            && let Some(pb) = running.get(id)
        {
            pb.set_message(format!(
                "{} (retrying: attempt {}/{})",
                id, attempt, max_retries
            ));
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
            TaskProgress::Log => eprintln!("{}", msg),
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

/// Writes one presenter line: results to stdout, everything else to stderr.
fn write_line(msg: &str, level: EmitLevel) {
    if level == EmitLevel::Result {
        println!("{msg}");
    } else {
        eprintln!("{msg}");
    }
}

fn write_stdout(bytes: &[u8]) {
    use std::io::Write as _;
    let mut out = std::io::stdout();
    let _ = out.write_all(bytes);
    let _ = out.flush();
}

/// Escapes control characters in each line, keeping the line breaks.
fn escape_control_chars_multiline(input: &str) -> String {
    input
        .split('\n')
        .map(|l| escape_control_chars(l).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Renders a notice as `warning[code]: message`, followed by a pointer to
/// `omni code explain`. Level names follow rustc: `error`, `warning`, `note`.
fn render_notice(e: &NoticeEvent, count: usize) -> String {
    let (label, color): (&str, fn(&str) -> String) = match e.level() {
        NoticeLevel::Error => ("error", |s| s.red().bold().to_string()),
        NoticeLevel::Warn => ("warning", |s| s.yellow().bold().to_string()),
        NoticeLevel::Info => ("note", |s| s.cyan().bold().to_string()),
    };
    let mut out = format!(
        "{}: {}",
        color(&format!("{label}[{}]", e.code())),
        escape_control_chars(e.message()),
    );
    if count > 1 {
        out.push_str(&format!(" (x{count})"));
    }
    if let Some(body) = e.body() {
        out.push('\n');
        out.push_str(&escape_control_chars_multiline(body.trim_end()));
    }
    if let Some(hints) = e.fields().get("hints").and_then(|h| h.as_array()) {
        for h in hints.iter().filter_map(|h| h.as_str()) {
            out.push_str(&format!("\n  = hint: {}", escape_control_chars(h)));
        }
    }
    out.push_str(&format!(
        "\n  = run `omni code explain {}` for details",
        e.code()
    ));
    out
}

fn notice_emit_level(level: NoticeLevel) -> EmitLevel {
    match level {
        NoticeLevel::Error => EmitLevel::Error,
        NoticeLevel::Warn => EmitLevel::Warn,
        NoticeLevel::Info => EmitLevel::Info,
    }
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
    /// Lifecycle lines buffered while the TUI owns the terminal, flushed once
    /// the UI is torn down (see `emit` and `flush_emit_buffer`). Each entry is
    /// `(message, is_error)`.
    emit_buffer: Arc<Mutex<Vec<(String, EmitLevel)>>>,
    /// `-q/--quiet`: a presenter setting only. Events and notices are still
    /// emitted and collected; this changes what is printed.
    quiet: bool,
    /// Notices waiting to be printed, collapsed by `(code, message)` in
    /// first-occurrence order.
    notice_groups: Arc<Mutex<Vec<NoticeGroup>>>,
    /// Print each notice as it arrives (commands with no run to flush at).
    immediate_notices: bool,
}

/// A run of identical notices (same code and redacted message).
struct NoticeGroup {
    event: NoticeEvent,
    count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitLevel {
    Info,
    Error,
    Warn,
    /// A result the user asked for (a dry-run command line). Goes to stdout
    /// and is never hidden by `-q`.
    Result,
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
            emit_buffer: Arc::new(Mutex::new(Vec::new())),
            quiet: false,
            notice_groups: Arc::new(Mutex::new(Vec::new())),
            immediate_notices: false,
        }
    }

    /// For one-shot commands that have no run to flush notices at: print each
    /// notice as it arrives.
    pub fn with_immediate_notices(mut self) -> Self {
        self.immediate_notices = true;
        self
    }

    /// Hide omni's own narration without suppressing events. Rendering only.
    pub fn with_quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
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
        self.mux.get_or_init(MuxOutputPresenterStatic::new_stream)
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
                if self.quiet || ProgressDrawTarget::stderr().is_hidden() {
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
        let quiet = self.quiet;
        f(guard
            .get_or_insert_with(move || GeneratorProgress::new(multi, quiet)));
    }

    /// Run `f` against the task progress renderer, if it has been seeded.
    fn with_task_progress(&self, f: impl FnOnce(&mut TaskProgress)) {
        let mut guard = self.task_progress.lock();
        if let Some(tp) = guard.as_mut() {
            f(tp);
        }
    }

    /// Run `f` (which writes to the terminal) coordinated with any live
    /// progress rendering (generator *or* task). Both renderers share the one
    /// `MultiProgress`, so suspending it clears whatever bars are currently
    /// drawn while `f` writes, then redraws them afterwards. When `indicatif`
    /// cannot render there is nothing to protect and `f` runs directly.
    fn suspend_for_output(&self, f: impl FnOnce()) {
        match self.shared_multi() {
            Some(multi) => multi.suspend(f),
            None => f(),
        }
    }

    /// Emit a lifecycle line. Under `-q` only failures print.
    fn emit(&self, msg: String, level: EmitLevel) {
        if self.quiet && !matches!(level, EmitLevel::Error | EmitLevel::Result)
        {
            return;
        }
        self.emit_always(msg, level);
    }

    /// Write a line to the terminal regardless of `-q`, routed per `--ui`
    /// mode: through the bar in `progress`, buffered while the TUI owns the
    /// terminal (flushed by `flush_emit_buffer`), directly to stderr
    /// otherwise.
    fn emit_always(&self, msg: String, level: EmitLevel) {
        match self.resolved_mode() {
            CliUiMode::Progress => {
                self.with_task_progress(|tp| tp.println(&msg));
            }
            // The TUI owns the terminal (alternate screen + raw mode), so
            // writing lines here would corrupt it. Buffer them and flush
            // once the TUI has been torn down.
            CliUiMode::Tui => {
                self.emit_buffer.lock().push((msg, level));
            }
            // A generator spinner may be live on the shared `MultiProgress`.
            _ => self.suspend_for_output(|| write_line(&msg, level)),
        }
    }

    /// Print every pending notice group. Redaction already happened in
    /// `publish_event`, and `-q` filtering in `on_notice`; this collapses
    /// repeats (`(xN)`) and renders in first-occurrence order.
    fn flush_notices(&self) {
        let groups: Vec<NoticeGroup> =
            std::mem::take(&mut *self.notice_groups.lock());
        for g in groups {
            self.emit_always(
                render_notice(&g.event, g.count),
                notice_emit_level(g.event.level()),
            );
        }
    }

    /// Like [`flush_notices`](Self::flush_notices), for after the TUI has been
    /// torn down: the terminal is free, so nothing is re-buffered.
    fn flush_notices_after_teardown(&self) {
        let groups: Vec<NoticeGroup> =
            std::mem::take(&mut *self.notice_groups.lock());
        for g in groups {
            let text = render_notice(&g.event, g.count);
            if self.resolved_mode() == CliUiMode::Progress {
                self.with_task_progress(|tp| tp.println(&text));
            } else {
                eprintln!("{text}");
            }
        }
    }

    /// Flush any lifecycle lines buffered while the TUI was live. Must be
    /// called only after the TUI has been torn down and the terminal restored
    /// (otherwise the lines would corrupt the alternate screen). No-op when
    /// nothing was buffered.
    fn flush_emit_buffer(&self) {
        for (msg, level) in self.emit_buffer.lock().drain(..) {
            write_line(&msg, level);
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
    #[allow(clippy::await_holding_lock)]
    pub async fn wait(&self) {
        let _ = self.get_mux().wait().await;

        let remaining: Vec<_> =
            self.captures.lock().drain().map(|(_, c)| c.drain).collect();
        for handle in remaining {
            let _ = handle.await;
        }

        let mut tasks = self.tasks.lock();
        while tasks.join_next().await.is_some() {}
        drop(tasks);

        // Belt-and-suspenders: on early-exit paths `on_execution_complete` may
        // never fire, leaving TUI-buffered lifecycle lines unflushed. Restore
        // the terminal (idempotent) and flush anything still buffered so it is
        // not lost. No-op when the buffer is already empty.
        self.get_mux().shutdown().await;
        self.flush_emit_buffer();
        self.flush_notices_after_teardown();
        omni_tracing_subscriber::resume_stderr();
    }
}

impl NoticeSubscriber for CliSubscriber {
    fn on_notice(&self, e: NoticeEvent) {
        // `-q` hides warn and info notices, never `error` or `essential`
        // ones. The level and `essential` come from the event, not a lookup.
        if self.quiet
            && !e.essential()
            && e.level() != omni_messages::NoticeLevel::Error
        {
            return;
        }
        let mut groups = self.notice_groups.lock();
        match groups.iter_mut().find(|g| {
            g.event.code() == e.code() && g.event.message() == e.message()
        }) {
            Some(g) => g.count += 1,
            None => groups.push(NoticeGroup { event: e, count: 1 }),
        }
        drop(groups);
        if self.immediate_notices {
            self.flush_notices_after_teardown();
        }
    }
}

/// Prints a notice raised outside an event run (a command with no
/// subscriber). Same layout as the run presenter.
pub fn print_notice<N: omni_notice::Notice>(n: &N, quiet: bool) {
    let e = NoticeEvent::from_notice(n, module_path!());
    // Like every published notice, it is recorded in the trace even when `-q`
    // hides it.
    omni_messages::publish::mirror_to_tracing(&e);
    if quiet && !e.essential() && e.level() != omni_messages::NoticeLevel::Error
    {
        return;
    }
    eprintln!("{}", render_notice(&e, 1));
}

impl ExecutionEventSubscriber for CliSubscriber {
    fn wants_task_output_stream(&self) -> bool {
        true
    }

    fn wants_task_input_stream(&self) -> bool {
        self.get_mux().accepts_input()
    }

    async fn on_task_started(&self, e: TaskStartedEvent) {
        tracing::debug!("Starting task '{}'", e.task_id);
        if let Some(command) = &e.command {
            self.emit(
                format!(
                    "Executing task '{}': {}",
                    escape_control_chars(&e.task_id),
                    command
                ),
                EmitLevel::Result,
            );
        }
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
                } else if self.quiet && self.mode == CliUiMode::Auto {
                    // `-q` never resolves `auto` to a progress bar.
                    CliUiMode::Stream
                } else {
                    CliUiMode::Progress
                }
            }
            CliUiMode::Stream => CliUiMode::Stream,
            CliUiMode::Tui => CliUiMode::Tui,
        };
        let _ = self.resolved_mode.set(resolved);

        // Initialise the multiplexer for the resolved mode, but only when it
        // was not already set at construction (explicit Stream/Tui). Merely
        // *constructing* `new_tui()` spawns a second `TuiPresenter` whose
        // `run_tui` loop grabs the terminal (raw mode + alternate screen); the
        // `set()` below then silently drops it because the `OnceLock` is
        // already populated, and that drop tears the terminal back down
        // (disable raw mode + leave alternate screen) out from under the
        // still-live presenter. The result is a visible-but-dead TUI: rendering
        // continues without raw mode, so keypresses never reach the active
        // task. Guard the construction itself, not just the `set`.
        if self.mux.get().is_none() {
            let presenter = if resolved == CliUiMode::Tui {
                MuxOutputPresenterStatic::new_tui()
            } else {
                MuxOutputPresenterStatic::new_stream()
            };
            let _ = self.mux.set(presenter);
        }

        // Seed the aggregate progress bar in progress mode.
        if resolved == CliUiMode::Progress {
            let multi = self.shared_multi();
            *self.task_progress.lock() =
                Some(TaskProgress::new(multi, e.total));
        }

        // A live bar or the alternate screen would be corrupted by stderr
        // traces; hold them until teardown (`--trace-file` is the route for
        // traces during such a run).
        if matches!(resolved, CliUiMode::Progress | CliUiMode::Tui)
            && !(self.quiet && resolved == CliUiMode::Progress)
        {
            omni_tracing_subscriber::pause_stderr();
        }

        if e.remote_cache {
            self.emit("Remote caching enabled".to_string(), EmitLevel::Info);
        }
        self.flush_notices();
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
                    mux.get_or_init(MuxOutputPresenterStatic::new_stream);
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
                    EmitLevel::Error,
                );
            }
        } else if !failed {
            self.emit(
                format!("Executed task '{}'", e.task_id).green().to_string(),
                EmitLevel::Info,
            );
        } else {
            self.emit(
                format!(
                    "Task '{}' failed with exit code {}",
                    e.task_id, e.exit_code
                )
                .red()
                .to_string(),
                EmitLevel::Error,
            );
        }
    }

    async fn on_task_failed(&self, e: TaskFailedEvent) {
        self.finish_capture(&e.task_id, true).await;
        self.emit(
            format!("Task '{}' error: {}", e.task_id, e.error)
                .red()
                .to_string(),
            EmitLevel::Error,
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
        self.emit(msg.white().dimmed().to_string(), EmitLevel::Info);
        self.with_task_progress(|tp| tp.task_finished(&e.task_id));
    }

    async fn on_task_retrying(&self, e: TaskRetryingEvent) {
        let wait = match e.delay {
            Some(d) if !d.is_zero() => format!(" in {d:?}"),
            _ => String::new(),
        };
        let msg = format!(
            "Task '{}' is retrying{wait}... (attempt {}/{})",
            e.task_id, e.attempt, e.max_retries
        );
        self.emit(msg.yellow().dimmed().to_string(), EmitLevel::Warn);
        self.with_task_progress(|tp| {
            tp.task_retrying(
                &e.task_id,
                e.attempt as usize,
                e.max_retries as usize,
            )
        });
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
        self.emit(msg, EmitLevel::Info);
        // Cache hits emit no terminal event, so advance the aggregate bar here.
        self.with_task_progress(|tp| tp.task_finished(&e.task_id));
    }

    async fn on_execution_complete(&self, e: ExecutionCompleteEvent) {
        // Flush all task output streams before printing the summary.
        _ = self.get_mux().wait().await;

        // Tear down the interactive UI (if any) so the terminal is restored
        // from the alternate screen before we print the summary table. Without
        // this the table is written into the about-to-be-discarded TUI screen
        // and never reaches the user. No-op outside TUI mode.
        self.get_mux().shutdown().await;

        // Now that the terminal is restored, flush any lifecycle lines that
        // were buffered while the TUI was live so they precede the summary.
        self.flush_emit_buffer();
        self.flush_notices_after_teardown();

        // Under `-q` the summary table is for failures only.
        if e.total > 0 && (!self.quiet || e.failed > 0) {
            let mut table = Table::new();
            const STYLE: TableStyle = UTF8_FULL.with_rounded_corners();
            table.load_style(STYLE).set_header(vec!["Result", "Count"]);

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

            // Render the table through the bar's `println` in progress mode so
            // it does not corrupt the spinner, otherwise straight to stderr:
            // the summary is omni's own status, not a command result, so it
            // stays off stdout.
            let rendered = table.to_string();
            if self.resolved_mode() == CliUiMode::Progress {
                self.with_task_progress(|tp| tp.println(&rendered));
            } else {
                eprintln!("{rendered}");
            }
        }

        // Tear down the aggregate bar once the summary has been printed.
        self.with_task_progress(|tp| tp.finish());
        omni_tracing_subscriber::resume_stderr();
    }

    async fn on_batch_completed(&self, _event: BatchCompletedEvent) {
        self.wait().await;
    }
}

impl GeneratorEventSubscriber for CliSubscriber {
    async fn on_prompt_rejected(
        &self,
        e: omni_messages::GeneratorPromptRejectedEvent,
    ) {
        self.emit_always(
            format!("error: {}", escape_control_chars(&e.message))
                .red()
                .to_string(),
            EmitLevel::Error,
        );
    }

    /// A script's own `ctx.log.*` output: `info`/`warn`/`error` are shown (a
    /// script author asked for them, so `-q` does not hide them); `debug` and
    /// `trace` go to `tracing` only.
    fn on_script_log(&self, e: omni_messages::ScriptLogEvent) {
        use omni_messages::ScriptLogLevel as L;
        e.mirror_to_tracing();
        let msg = escape_control_chars(&e.message).into_owned();
        match e.level {
            L::Info => self.emit_always(msg, EmitLevel::Info),
            L::Warn => {
                self.emit_always(msg.yellow().to_string(), EmitLevel::Warn)
            }
            L::Error => {
                self.emit_always(msg.red().to_string(), EmitLevel::Error)
            }
            L::Debug | L::Trace => {}
        }
    }

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
        // Notices from sub-generators print too, so flush at every depth.
        self.flush_notices();
    }

    async fn on_action_failed(&self, e: GeneratorActionFailedEvent) {
        if e.depth == 0 {
            self.with_progress(|p| p.action_failed(&e.name, &e.message));
        }
        self.flush_notices();
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
        self.flush_notices();
    }
}

#[cfg(test)]
mod notice_render_tests {
    use omni_messages::NoticeEvent;
    use omni_notice::Notice;

    use super::*;

    omni_codes::prefixes!("clitest");

    /// Sample.
    #[derive(Notice)]
    #[notice(code = "clitest.sample", level = warn, message = "line\nbreak {x}")]
    struct Sample {
        x: String,
    }

    fn plain(s: &str) -> String {
        // Strip ANSI colour so assertions do not depend on the terminal.
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for n in chars.by_ref() {
                    if n == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Essential sample.
    #[derive(Notice)]
    #[notice(code = "clitest.essential", level = warn, essential, message = "static")]
    struct Essential;

    /// Error sample.
    #[derive(Notice)]
    #[notice(code = "clitest.error", level = error, message = "failed {x}")]
    struct Failed {
        x: u8,
    }

    fn groups(sub: &CliSubscriber) -> Vec<(String, usize)> {
        sub.notice_groups
            .lock()
            .iter()
            .map(|g| (g.event.code().to_string(), g.count))
            .collect()
    }

    fn sample(x: &str) -> NoticeEvent {
        NoticeEvent::from_notice(&Sample { x: x.into() }, "t")
    }

    #[test]
    fn identical_notices_collapse_in_first_occurrence_order() {
        let sub = CliSubscriber::new_stream(std::env::temp_dir());
        sub.on_notice(sample("a"));
        sub.on_notice(NoticeEvent::from_notice(&Failed { x: 1 }, "t"));
        sub.on_notice(sample("a"));
        sub.on_notice(sample("b"));
        sub.on_notice(sample("a"));

        assert_eq!(
            groups(&sub),
            vec![
                ("clitest.sample".to_string(), 3),
                ("clitest.error".to_string(), 1),
                ("clitest.sample".to_string(), 1),
            ]
        );
    }

    #[test]
    fn the_count_suffix_follows_the_message_and_a_single_has_none() {
        let e = sample("a");
        assert!(
            plain(&render_notice(&e, 3))
                .starts_with("warning[clitest.sample]: line\\nbreak a (x3)")
        );
        assert!(!plain(&render_notice(&e, 1)).contains("(x"));
    }

    #[test]
    fn quiet_hides_warn_but_not_error_or_essential() {
        let sub =
            CliSubscriber::new_stream(std::env::temp_dir()).with_quiet(true);
        sub.on_notice(sample("a"));
        sub.on_notice(NoticeEvent::from_notice(&Failed { x: 1 }, "t"));
        sub.on_notice(NoticeEvent::from_notice(&Essential, "t"));

        let codes: Vec<String> =
            groups(&sub).into_iter().map(|(c, _)| c).collect();
        assert_eq!(codes, vec!["clitest.error", "clitest.essential"]);
    }

    #[test]
    fn a_notice_published_while_the_tui_is_live_is_buffered_not_printed() {
        let sub = CliSubscriber::new_with(
            CliUiMode::Tui,
            std::env::temp_dir(),
            Arc::new(OnceLock::new()),
        );
        let _ = sub.resolved_mode.set(CliUiMode::Tui);

        sub.on_notice(sample("a"));
        sub.flush_notices();

        // Held for `flush_emit_buffer` after teardown, never written to the
        // terminal that the alternate screen owns.
        let buffered = sub.emit_buffer.lock();
        assert_eq!(buffered.len(), 1);
        assert!(buffered[0].0.contains("clitest.sample"));
        assert!(sub.notice_groups.lock().is_empty());
    }

    #[test]
    fn multiline_escaping_keeps_line_breaks_but_drops_escapes() {
        let out = escape_control_chars_multiline("a\x1b[31mb\nc\rd");
        assert_eq!(out.matches('\n').count(), 1);
        assert!(!out.contains('\x1b'));
        assert!(!out.contains('\r'));
    }

    #[test]
    fn script_log_is_redacted_by_the_operation_before_it_is_shown() {
        let sub = CliSubscriber::new_with(
            CliUiMode::Tui,
            std::env::temp_dir(),
            Arc::new(OnceLock::new()),
        );
        let _ = sub.resolved_mode.set(CliUiMode::Tui);
        let op = omni_messages::OperationPublisher::new(
            &sub,
            omni_redact::Redactor::builder().value("hunter2").build(),
        );
        op.on_script_log(omni_messages::ScriptLogEvent {
            level: omni_messages::ScriptLogLevel::Info,
            target: "t".into(),
            message: "pw hunter2".into(),
        });
        let out = sub.emit_buffer.lock();
        assert_eq!(out.len(), 1);
        assert!(!out[0].0.contains("hunter2"));
    }

    #[test]
    fn a_secret_of_the_operation_is_redacted_before_collapsing() {
        let sub = CliSubscriber::new_stream(std::env::temp_dir());
        let op = omni_messages::OperationPublisher::new(
            &sub,
            omni_redact::Redactor::builder().value("hunter2").build(),
        );

        omni_messages::publish::publish_event(&op, sample("hunter2"));
        omni_messages::publish::publish_event(&op, sample("hunter2"));

        let groups = sub.notice_groups.lock();
        assert_eq!(groups.len(), 1, "same text once redacted");
        assert_eq!(groups[0].count, 2);
        assert!(!groups[0].event.message().contains("hunter2"));
        assert!(
            !groups[0].event.fields()["x"]
                .as_str()
                .unwrap()
                .contains("hunter2")
        );
    }

    #[test]
    fn not_quiet_keeps_everything() {
        let sub = CliSubscriber::new_stream(std::env::temp_dir());
        sub.on_notice(sample("a"));
        assert_eq!(groups(&sub).len(), 1);
    }

    #[test]
    fn essential_is_read_from_the_event_and_matches_the_registry() {
        let e = NoticeEvent::from_notice(&Essential, "t");
        assert!(e.essential());
        assert!(omni_codes::get(e.code()).unwrap().essential());
    }

    #[test]
    fn renders_level_code_message_and_code_explain_pointer() {
        let e = NoticeEvent::from_notice(&Sample { x: "v".into() }, "t");
        let text = plain(&render_notice(&e, 1));
        assert!(text.starts_with("warning[clitest.sample]: "));
        // Control characters in the message cannot forge extra lines.
        assert!(text.contains("line\\nbreak v"));
        assert!(
            text.ends_with(
                "= run `omni code explain clitest.sample` for details"
            )
        );
    }
}
