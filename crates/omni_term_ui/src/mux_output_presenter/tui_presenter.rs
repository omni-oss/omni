use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use crossterm::{
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode,
    },
};
use derive_new::new;
use futures::future::try_join_all;
use maps::{Map, UnorderedMap};
use parking_lot::RwLock;
use ratatui::{
    Frame, Terminal,
    crossterm::event::{
        self, Event as CEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    },
    layout::{Constraint, Direction, Layout, Margin},
    prelude::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex as AsyncMutex, RwLock as AsyncRwLock, mpsc, oneshot},
    task::JoinHandle,
};
use tracing::dispatcher;

use crate::mux_output_presenter::{
    MuxOutputPresenter, MuxOutputPresenterReader, MuxOutputPresenterWriter,
    StreamHandle,
    scrollable_tabs::ScrollableTabs,
    stream,
    stream_driver_handle::StreamDriverError,
    task_screen::{
        ScreenAction, ScreenActionsKind, TaskScreen, TaskScreenStatus,
    },
    utils::TasksMap,
};

type ShutdownTx = Arc<AsyncMutex<Option<oneshot::Sender<()>>>>;
type Screens = Arc<RwLock<Map<String, TaskScreen>>>;
type ActiveId = Arc<RwLock<Option<String>>>;
type InputHandle = Box<dyn MuxOutputPresenterWriter>;
type Inputs = Arc<AsyncMutex<UnorderedMap<String, InputHandle>>>;

pub struct TuiPresenter {
    screens: Screens,
    tasks: Arc<AsyncMutex<TasksMap<TuiPresenterError>>>,
    inputs: Inputs,
    inputs_task: JoinHandle<Result<(), TuiPresenterError>>,
    ui_task: Arc<AsyncRwLock<Option<JoinHandle<()>>>>,
    ui_shutdown_tx: ShutdownTx,
}

impl TuiPresenter {
    pub fn new() -> Self {
        let screens = Arc::new(RwLock::new(Map::default()));
        let tasks = Arc::new(AsyncMutex::new(TasksMap::default()));
        let active_id = Arc::new(RwLock::new(None));
        let inputs = Arc::new(AsyncMutex::new(UnorderedMap::<
            String,
            InputHandle,
        >::default()));
        let (keys_tx, mut keys_rx) = mpsc::unbounded_channel();

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // clone for UI task
        let ui_buffers = screens.clone();

        // spawn the UI loop in a task
        let ui_active_id = active_id.clone();
        let dispatch = dispatcher::get_default(|d| d.clone());
        let ui = tokio::task::spawn_blocking(move || {
            dispatcher::with_default(&dispatch, || {
                if let Err(e) =
                    run_tui(ui_active_id, ui_buffers, shutdown_rx, keys_tx)
                {
                    trace::error!("TUI exited with error: {:?}", e);
                }
            })
        });

        let i_inputs = inputs.clone();
        let inputs_task = tokio::spawn(async move {
            while let Some(key) = keys_rx.recv().await {
                trace::trace!(
                    "received_key: {} => {:?}",
                    key.id,
                    key.key_event
                );

                let mut uts = i_inputs.lock().await;
                let mut input = uts.get_mut(&key.id);
                if let Some(w) = input.as_mut() {
                    let bytes = key_event_to_bytes(key.key_event);
                    trace::trace!("found input, sending bytes: {:?}", bytes);
                    w.write_all(&bytes).await?;
                }
            }
            Ok(())
        });

        Self {
            screens,
            tasks,
            ui_shutdown_tx: Arc::new(AsyncMutex::new(Some(shutdown_tx))),
            ui_task: Arc::new(AsyncRwLock::new(Some(ui))),
            inputs,
            inputs_task,
        }
    }
}

#[async_trait::async_trait]
impl MuxOutputPresenter for TuiPresenter {
    type Error = TuiPresenterError;

    async fn add_stream(
        &self,
        id: String,
        output: Box<dyn MuxOutputPresenterReader>,
        input: Option<Box<dyn MuxOutputPresenterWriter>>,
    ) -> Result<StreamHandle, Self::Error> {
        let (handle, driver) = stream::handle();
        let (screen_actions_tx, screen_actions_rx) =
            crossbeam_channel::unbounded();

        // prepare buffer
        let screen = TaskScreen::new(id.clone(), screen_actions_rx);
        trace::trace!("{id}: buffer created");
        self.screens.write().insert(id.clone(), screen);
        trace::trace!("{id}: buffer inserted");
        if let Some(input) = input {
            self.inputs.lock().await.insert(id.clone(), input);
            trace::trace!("{id}: input inserted");
        }

        // spawn a task that copies reader -> buffer via BufferWriter
        let join_handle: JoinHandle<Result<(), TuiPresenterError>> = {
            let t_tasks = self.tasks.clone();
            let t_id = id.clone();
            let t_ui_task = self.ui_task.clone();
            let t_screens = self.screens.clone();
            let t_inputs = self.inputs.clone();
            let mut t_reader = output;

            tokio::spawn(async move {
                let mut buff = [0u8; 4096];

                let mut in_progress_status_sent = false;

                while !driver.is_stopped()
                    && let Ok(n) = t_reader.read(&mut buff).await
                    && n > 0
                {
                    // still consume data if UI task is not running anymore
                    if let Some(t) = t_ui_task.read().await.as_ref()
                        && !t.is_finished()
                    {
                        if !in_progress_status_sent {
                            screen_actions_tx.send(
                                ScreenAction::UpdateStatus(
                                    TaskScreenStatus::InProgress,
                                ),
                            ).map_err(|e| TuiPresenterErrorInner::new_failed_to_send_action(ScreenActionsKind::UpdateStatus, e))?;
                            in_progress_status_sent = true;
                        }

                        screen_actions_tx.send(ScreenAction::Write(
                            Bytes::copy_from_slice(&buff[..n]),
                        )).map_err(|e| TuiPresenterErrorInner::new_failed_to_send_action(ScreenActionsKind::Write, e))?;
                    } else {
                        trace::trace!(
                            "{t_id}: UI task is not running anymore, ignoring data"
                        );
                    }
                }

                screen_actions_tx
                    .send(ScreenAction::UpdateStatus(
                        TaskScreenStatus::Complete,
                    ))
                    .map_err(|e| {
                        TuiPresenterErrorInner::new_failed_to_send_action(
                            ScreenActionsKind::UpdateStatus,
                            e,
                        )
                    })?;

                t_tasks.lock().await.remove(&t_id);
                trace::trace!("{t_id}: task removed");
                t_screens.write().shift_remove(&t_id);
                trace::trace!("{t_id}: screen removed");
                t_inputs.lock().await.remove(&t_id);
                trace::trace!("{t_id}: input removed");

                // signal driver completion
                driver.mark_completed().await?;
                trace::trace!("{t_id}: completed");

                Ok(())
            })
        };

        self.tasks.lock().await.insert(id.clone(), join_handle);
        trace::trace!("{id}: task inserted");
        Ok(handle)
    }

    #[inline(always)]
    fn accepts_input(&self) -> bool {
        true
    }

    async fn wait(&self) -> Result<(), Self::Error> {
        wait(&self.tasks).await?;
        Ok(())
    }

    async fn close(self) -> Result<(), Self::Error> {
        // signal UI shutdown
        if let Some(tx) = self.ui_shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }

        let ui_task = self.ui_task.write().await.take();

        if let Some(ui_task) = ui_task
            && !ui_task.is_finished()
            && let Err(e) = ui_task.await
        {
            trace::error!("UI task exited with error: {:?}", e);
        }
        wait(&self.tasks.clone()).await?;
        self.inputs_task.await??;
        Ok(())
    }
}

async fn wait(
    tasks: &Arc<AsyncMutex<TasksMap<TuiPresenterError>>>,
) -> Result<(), TuiPresenterError> {
    let all_values = {
        let mut tasks = tasks.lock().await;
        if tasks.is_empty() {
            return Ok(());
        }
        tasks.drain().map(|(_, j)| j).collect::<Vec<_>>()
    };
    let values = try_join_all(all_values).await?;
    for value in values {
        value?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct InputEvent {
    key_event: KeyEvent,
    id: String,
}

#[derive(Debug, Copy, Clone)]
struct ScrollState {
    pub scroll_y: usize,
    pub follow: bool,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            scroll_y: 0,
            follow: true,
        }
    }
}

fn run_tui(
    active_id: ActiveId,
    screens: Screens,
    mut shutdown_rx: oneshot::Receiver<()>,
    keys_tx: mpsc::UnboundedSender<InputEvent>,
) -> eyre::Result<()> {
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();
    let mut scroll_states = UnorderedMap::default();
    let mut input_enabled = false;
    let mut scrollbar_state = ScrollbarState::default();

    // Setup terminal (must be done on a thread that can manipulate stdout).
    // Use spawn_blocking to avoid blocking the tokio runtime thread on synchronous crossterm setup/reads.
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;

    // Create terminal backend
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    loop {
        if let Ok(_) = shutdown_rx.try_recv() {
            trace::trace!("shutdown requested");
            shutdown_rx.close();
            break;
        }
        trace::trace!("scroll states: {:?}", scroll_states);

        let fd =
            get_frame_data(&screens, &active_id, input_enabled, &scroll_states);

        let order = fd.order.clone();
        let active_index = fd.active_index;
        let line_count = fd.line_count;
        let acting_active_id = fd.active_id.clone();
        let _ = terminal.draw(|f| {
            let state = draw_ui(f, &mut scrollbar_state, fd);

            if let Some(active_id) = acting_active_id.as_deref()
                && let Some(scroll_state) = scroll_states.get_mut(active_id)
            {
                trace::trace!(
                    "State for {active_id:?}: {scroll_state:?}, {state:?}"
                );
                scroll_state.scroll_y = state.paragraph_scroll_y;
            }
        })?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if event::poll(timeout)? {
            let ev = event::read()?;
            trace::trace!("polled event: {:?}", ev);
            if let CEvent::Key(key) = ev
                && key.kind == KeyEventKind::Press
            {
                match (key.modifiers, key.code) {
                    (KeyModifiers::CONTROL, KeyCode::Char('z')) => {
                        input_enabled = !input_enabled;
                    }
                    (_, code) if !input_enabled => match code {
                        KeyCode::Char('f') => {
                            update_or_insert_scroll_state(
                                acting_active_id.as_deref(),
                                &mut scroll_states,
                                |scroll_state| {
                                    scroll_state.scroll_y = line_count;
                                    scroll_state.follow = !scroll_state.follow;
                                },
                                || ScrollState {
                                    scroll_y: line_count,
                                    follow: false,
                                },
                            );
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            trace::trace!("Down key pressed");
                            update_or_insert_scroll_state(
                                acting_active_id.as_deref(),
                                &mut scroll_states,
                                |scroll_state| {
                                    scroll_state.scroll_y =
                                        scroll_state.scroll_y.saturating_add(1);
                                    scroll_state.follow = false;
                                },
                                || ScrollState {
                                    scroll_y: 1,
                                    follow: false,
                                },
                            );
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            trace::trace!("Up key pressed");
                            update_or_insert_scroll_state(
                                acting_active_id.as_deref(),
                                &mut scroll_states,
                                |scroll_state| {
                                    scroll_state.scroll_y =
                                        scroll_state.scroll_y.saturating_sub(1);
                                    scroll_state.follow = false;
                                },
                                || ScrollState {
                                    scroll_y: line_count.saturating_sub(1),
                                    follow: false,
                                },
                            );
                        }
                        KeyCode::Left | KeyCode::Char('l') => {
                            let (new_active_id, _idx) = compute_active(
                                active_index,
                                &order,
                                Compute::Prev,
                            );
                            *active_id.write() = new_active_id.cloned();
                        }
                        KeyCode::Right | KeyCode::Char('h') => {
                            let (new_active_id, _idx) = compute_active(
                                active_index,
                                &order,
                                Compute::Next,
                            );
                            *active_id.write() = new_active_id.cloned();
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            trace::trace!(
                                "received ESC or q, shutdown requested"
                            );
                            break;
                        }
                        _ => {
                            trace::trace!("no events matching")
                        }
                    },
                    _ => {
                        if input_enabled
                            && let Some(active_id) = acting_active_id.as_deref()
                        {
                            let input_event = InputEvent {
                                key_event: key,
                                id: active_id.to_string(),
                            };
                            keys_tx.send(input_event)?;
                        } else {
                            trace::trace!("no events matching")
                        }
                    }
                }
            }
        }

        trace::trace!("ui drawn");

        trace::trace!("looped");
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    trace::trace!("ui loop exited");

    disable_raw_mode()?;
    execute!(
        std::io::stdout(),
        Clear(ClearType::Purge),
        LeaveAlternateScreen
    )?;

    trace::trace!("stdout cleanup done");

    trace::trace!("exiting ui loop");
    Ok(())
}

fn update_or_insert_scroll_state(
    active_id: Option<&str>,
    scroll_states: &mut UnorderedMap<String, ScrollState>,
    mut update_fn: impl FnMut(&mut ScrollState),
    insert_fn: impl FnOnce() -> ScrollState,
) -> Option<ScrollState> {
    if let Some(scroll_state) =
        get_current_scroll_state(active_id, scroll_states)
    {
        trace::trace!(
            "Current scroll state for {active_id:?}: {scroll_state:?}"
        );
        update_fn(scroll_state);
        trace::trace!(
            "Updated scroll state for {active_id:?}, new value: {scroll_state:?}"
        );
        return Some(*scroll_state);
    } else if let Some(active_id) = active_id.as_deref() {
        let scroll_state = insert_fn();

        scroll_states.insert(active_id.to_string(), scroll_state);
        trace::trace!(
            "Inserted scroll state for {active_id:?}, new value: {scroll_state:?}"
        );
        return Some(scroll_state);
    }

    None
}

fn get_current_scroll_state<'a>(
    active_id: Option<&str>,
    scroll_states: &'a mut UnorderedMap<String, ScrollState>,
) -> Option<&'a mut ScrollState> {
    if let Some(id) = active_id {
        scroll_states.get_mut(id)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
enum Compute {
    Next,
    Prev,
}

fn compute_active<'a>(
    current_index: usize,
    order: &'a [String],
    compute: Compute,
) -> (Option<&'a String>, usize) {
    let order_len = order.len();
    if order_len == 0 {
        return (None, 0);
    }

    let proposed_index: i32 = match compute {
        Compute::Next => current_index as i32 + 1,
        Compute::Prev => current_index as i32 - 1,
    };

    let active_index = if proposed_index < 0 {
        order_len - 1
    } else {
        proposed_index as usize % order_len
    };

    (order.get(active_index), active_index)
}

struct FrameData {
    active_index: usize,
    active_id: Option<String>,
    order: Vec<String>,
    paragraph: Paragraph<'static>,
    line_count: usize,
    scroll_state: ScrollState,
    input_enabled: bool,
}

fn get_active_index(order: &[String], active_id: Option<&str>) -> usize {
    let f_active_id = active_id;

    let active_index = if let Some(id) = f_active_id {
        order.iter().position(|s| s == &id).unwrap_or(0)
    } else {
        0
    };

    active_index
}

fn get_frame_data<'a>(
    buffers: &Screens,
    active_id: &ActiveId,
    input_enabled: bool,
    scroll_states: &UnorderedMap<String, ScrollState>,
) -> FrameData {
    let order = buffers.read().keys().rev().cloned().collect::<Vec<_>>();

    let active_id = active_id.read();
    trace::trace!("active id: {active_id:?}");
    let active_index = get_active_index(&order, active_id.as_deref());
    let active_id = order.get(active_index);

    let mut buffers = buffers.write();
    let active_screen = if let Some(id) = order.get(active_index)
        && let Some(buf) = buffers.get_mut(id)
    {
        Some(buf)
    } else {
        None
    };

    fn block<'a, T: Into<Line<'a>>>(title: T) -> Block<'a> {
        Block::new().title(title).borders(Borders::ALL)
    }

    let (paragraph, line_count) = if let Some(active_screen) = active_screen {
        active_screen.apply_pending_actions();

        let (paragraph, line_count) = active_screen.paragraph();

        (
            paragraph.block(block(format!(
                " Output - {} ({}) ",
                active_screen.title, active_screen.status
            ))),
            line_count,
        )
    } else {
        (Paragraph::new("<no data>").block(block(" Output ")), 1)
    };

    let scroll_state = if let Some(id) = active_id.as_deref() {
        trace::trace!("scroll state for {id}: {:?}", scroll_states.get(id));
        scroll_states.get(id).copied().unwrap_or_default()
    } else {
        trace::trace!("no scroll state for active id: {active_id:?}");
        ScrollState::default()
    };

    FrameData {
        active_index,
        active_id: active_id.cloned(),
        order,
        paragraph,
        line_count,
        scroll_state,
        input_enabled,
    }
}

#[derive(Debug, Clone, Copy)]
struct DrawState {
    paragraph_scroll_y: usize,
    #[allow(unused)]
    paragraph_vp_height: usize,
}

/// Draw UI frame: left vertical tab list + right content for selected stream
fn draw_ui<'a>(
    f: &mut Frame<'a>,
    scroll_bar_state: &mut ScrollbarState,
    FrameData {
        active_index,
        order,
        paragraph,
        line_count,
        scroll_state,
        input_enabled,
        active_id: _,
    }: FrameData,
) -> DrawState {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(10)].as_ref())
        .split(area);

    let items: Vec<Line> = order.iter().map(|s| Line::raw(s)).collect();

    let tabs = ScrollableTabs::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .padding(" ", " ")
        .select(active_index);

    f.render_widget(tabs, chunks[0]);

    // right pane
    let right_pane_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)].as_ref())
        .split(chunks[1]);

    // terminal output
    let vp_height = right_pane_chunks[0].height.saturating_sub(2); // remove the borders

    let max_scroll_y = line_count.saturating_sub(vp_height as usize);
    let scroll_y = if scroll_state.follow {
        max_scroll_y
    } else {
        scroll_state.scroll_y.min(max_scroll_y)
    };

    if line_count > vp_height as usize {
        *scroll_bar_state = scroll_bar_state
            .content_length(line_count)
            .position(scroll_y.into());

        // show a scroll bar
        let scroll_bar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        let output_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [Constraint::Length(vp_height), Constraint::Min(0)].as_ref(),
            )
            .split(right_pane_chunks[0]);
        f.render_widget(
            paragraph.scroll((scroll_y as u16, 0)),
            right_pane_chunks[0],
        );
        f.render_stateful_widget(
            scroll_bar,
            output_chunks[1].inner(Margin {
                horizontal: 0,
                vertical: 1,
            }),
            scroll_bar_state,
        );
    } else {
        f.render_widget(
            paragraph.scroll((scroll_y as u16, 0)),
            right_pane_chunks[0],
        );
    }

    // controls
    const CONTROL_STYLE: Style = Style::new()
        .fg(Color::White)
        .bg(Color::Blue)
        .add_modifier(Modifier::BOLD);

    let controls = if input_enabled {
        vec![Span::styled(
            " ctrl+z = Disable Input ",
            CONTROL_STYLE.clone(),
        )]
    } else {
        vec![
            Span::styled(" ESC / q = Quit ", CONTROL_STYLE.clone()),
            Span::raw(" • "),
            if scroll_state.follow {
                Span::styled(" f = Unfollow Scroll ", CONTROL_STYLE.clone())
            } else {
                Span::styled(" f = Follow Scroll ", CONTROL_STYLE.clone())
            },
            Span::raw(" • "),
            Span::styled(" ⟵ ⟶ / h-l = Select Task ", CONTROL_STYLE.clone()),
            Span::raw(" • "),
            Span::styled(" ↑ ↓ / j-k = Scroll Up/Down ", CONTROL_STYLE.clone()),
            Span::raw(" • "),
            Span::styled(" ctrl+z = Enable Input ", CONTROL_STYLE.clone()),
        ]
    };

    let control_paragraph = Paragraph::new(Line::from(controls));
    f.render_widget(control_paragraph, right_pane_chunks[1]);

    DrawState {
        paragraph_scroll_y: scroll_y,
        paragraph_vp_height: vp_height as usize,
    }
}

fn key_event_to_bytes(ev: KeyEvent) -> Vec<u8> {
    use KeyCode::*;
    use KeyModifiers as M;

    let mut out = Vec::<u8>::new();

    // helper: prepend ESC for Alt-modified sequences
    let mut push_with_alt = |bytes: &[u8]| {
        if ev.modifiers.contains(M::ALT) {
            out.push(0x1B);
        }
        out.extend_from_slice(bytes);
    };

    match ev.code {
        Char(c) => {
            // Ctrl+char -> control code (e.g. Ctrl-A = 0x01)
            if ev.modifiers.contains(M::CONTROL) {
                let lc = (c as u8).to_ascii_lowercase();
                // common mapping for letters and a few symbols
                let ctrl = match lc {
                    b'@' => 0x00,
                    b'a'..=b'z' => lc & 0x1F,
                    b'[' => 0x1B, // Ctrl-[ = ESC
                    b'\\' => 0x1C,
                    b']' => 0x1D,
                    b'^' => 0x1E,
                    b'_' => 0x1F,
                    _ => lc & 0x1F, // best-effort
                };
                out.push(ctrl);
            } else {
                // normal char (respect Shift in the char value already)
                if ev.modifiers.contains(M::ALT) {
                    out.push(0x1B);
                }
                let mut buf = [0; 4];
                let s = c.encode_utf8(&mut buf);
                out.extend_from_slice(s.as_bytes());
            }
        }

        Enter => out.push(b'\r'),
        Tab => out.push(b'\t'),
        Backspace => out.push(0x7f), // DEL commonly used
        Esc => out.push(0x1B),

        Left => push_with_alt(b"\x1b[D"),
        Right => push_with_alt(b"\x1b[C"),
        Up => push_with_alt(b"\x1b[A"),
        Down => push_with_alt(b"\x1b[B"),

        Home => push_with_alt(b"\x1b[H"),
        End => push_with_alt(b"\x1b[F"),
        PageUp => push_with_alt(b"\x1b[5~"),
        PageDown => push_with_alt(b"\x1b[6~"),
        Insert => push_with_alt(b"\x1b[2~"),
        Delete => push_with_alt(b"\x1b[3~"),

        F(n) => {
            // rough, commonly-used function key sequences
            let seq: &[u8] = match n {
                1 => b"\x1bOP",
                2 => b"\x1bOQ",
                3 => b"\x1bOR",
                4 => b"\x1bOS",
                5 => b"\x1b[15~",
                6 => b"\x1b[17~",
                7 => b"\x1b[18~",
                8 => b"\x1b[19~",
                9 => b"\x1b[20~",
                10 => b"\x1b[21~",
                11 => b"\x1b[23~",
                12 => b"\x1b[24~",
                _ => b"",
            };
            push_with_alt(seq);
        }

        // numeric keypad / other keys not covered above
        Null => {}
        _ => {}
    }

    out
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct TuiPresenterError(pub(crate) TuiPresenterErrorInner);

impl TuiPresenterError {
    #[allow(unused)]
    pub fn kind(&self) -> TuiPresenterErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<TuiPresenterErrorInner>> From<T> for TuiPresenterError {
    fn from(e: T) -> Self {
        let inner = e.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(TuiPresenterErrorKind))]
pub(crate) enum TuiPresenterErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),

    #[error(transparent)]
    StreamDriver(#[from] StreamDriverError),

    #[error("failed to send action: {0:?}")]
    FailedToSendAction(
        ScreenActionsKind,
        #[source] crossbeam_channel::SendError<ScreenAction>,
    ),
}
