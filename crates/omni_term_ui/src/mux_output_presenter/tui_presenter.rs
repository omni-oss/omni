use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use crossterm::{
    event::{self, Event as CEvent, KeyCode},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode,
    },
};
use derive_new::new;
use futures::future::try_join_all;
use maps::Map;
use parking_lot::Mutex;
use ratatui::{
    Frame, Terminal,
    layout::{Constraint, Direction, Layout},
    prelude::CrosstermBackend,
    style::{Modifier, Style},
    text::Text,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::{
    io::AsyncReadExt,
    sync::{Mutex as AsyncMutex, RwLock, oneshot},
    task::JoinHandle,
};

use crate::mux_output_presenter::{
    MuxOutputPresenter, MuxOutputPresenterReader, MuxOutputPresenterWriter,
    StreamHandle, stream,
    stream_driver_handle::StreamDriverError,
    task_screen::{
        ScreenAction, ScreenActionsKind, TaskScreen, TaskScreenStatus,
    },
    utils::TasksMap,
};

type ShutdownTx = Arc<AsyncMutex<Option<oneshot::Sender<()>>>>;
type Screens = Arc<Mutex<Map<String, TaskScreen>>>;
type ActiveId = Arc<Mutex<Option<String>>>;

pub struct TuiPresenter {
    screens: Screens,
    tasks: Arc<AsyncMutex<TasksMap<TuiPresenterError>>>,
    ui_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    ui_shutdown_tx: ShutdownTx,
    input_writer: Arc<AsyncMutex<Option<Box<dyn MuxOutputPresenterWriter>>>>,
}

impl TuiPresenter {
    pub fn new() -> Self {
        let screens = Arc::new(Mutex::new(Map::default()));
        let tasks = Arc::new(AsyncMutex::new(TasksMap::default()));
        let active_id = Arc::new(Mutex::new(None));

        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // clone for UI task
        let ui_buffers = screens.clone();

        trace::warn!("Tui mode is currently experimental");

        // spawn the UI loop in a task
        let ui_active_id = active_id.clone();
        let ui = tokio::task::spawn_blocking(move || {
            if let Err(e) = run_tui(ui_active_id, ui_buffers, shutdown_rx) {
                trace::error!("TUI exited with error: {:?}", e);
            }
        });

        Self {
            screens,
            tasks,
            ui_shutdown_tx: Arc::new(AsyncMutex::new(Some(shutdown_tx))),
            input_writer: Arc::new(AsyncMutex::new(None)), // new
            ui_task: Arc::new(RwLock::new(Some(ui))),
        }
    }
}

#[async_trait::async_trait]
impl MuxOutputPresenter for TuiPresenter {
    type Error = TuiPresenterError;

    async fn add_stream(
        &self,
        id: String,
        reader: Box<dyn MuxOutputPresenterReader>,
    ) -> Result<StreamHandle, Self::Error> {
        let (handle, driver) = stream::handle();
        let (screen_actions_tx, screen_actions_rx) =
            crossbeam_channel::unbounded();

        // prepare buffer
        let screen = TaskScreen::new(id.clone(), screen_actions_rx);
        trace::debug!("{id}: buffer created");
        self.screens.lock().insert(id.clone(), screen);
        trace::debug!("{id}: buffer inserted");

        // spawn a task that copies reader -> buffer via BufferWriter
        let join_handle: JoinHandle<Result<(), TuiPresenterError>> = {
            let t_tasks = self.tasks.clone();
            let t_id = id.clone();
            let t_ui_task = self.ui_task.clone();
            let t_screens = self.screens.clone();
            let mut t_reader = reader;

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
                        trace::debug!(
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
                trace::debug!("{t_id}: task removed");
                t_screens.lock().shift_remove(&t_id);
                trace::debug!("{t_id}: screen removed");

                // signal driver completion
                driver.mark_completed().await?;
                trace::debug!("{t_id}: completed");

                Ok(())
            })
        };

        self.tasks.lock().await.insert(id.clone(), join_handle);
        trace::debug!("{id}: task inserted");
        Ok(handle)
    }

    #[inline(always)]
    async fn register_input_writer(
        &self,
        writer: Box<dyn MuxOutputPresenterWriter>,
    ) -> Result<(), Self::Error> {
        *self.input_writer.lock().await = Some(writer);
        Ok(())
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

fn run_tui(
    active_id: ActiveId,
    screens: Screens,
    mut shutdown_rx: oneshot::Receiver<()>,
) -> eyre::Result<()> {
    let tick_rate = Duration::from_millis(250);
    let mut last_tick = Instant::now();

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
            trace::debug!("shutdown requested");
            shutdown_rx.close();
            break;
        }
        let mut fd = get_frame_data(&screens, &active_id);

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());

        if event::poll(timeout)? {
            let ev = event::read()?;
            trace::debug!("polled event: {:?}", ev);
            if let CEvent::Key(key) = ev {
                match key.code {
                    KeyCode::Up => {
                        let (new_active_id, new_active_index) =
                            compute_active(&fd, Compute::Prev);
                        *active_id.lock() = new_active_id;
                        fd.active_index = new_active_index;
                    }
                    KeyCode::Down => {
                        let (new_active_id, new_active_index) =
                            compute_active(&fd, Compute::Next);
                        *active_id.lock() = new_active_id;
                        fd.active_index = new_active_index;
                    }
                    KeyCode::Char('q') | KeyCode::Esc => {
                        trace::debug!("received ESC or q, shutdown requested");
                        break;
                    }
                    _ => {
                        trace::debug!("no key matched, sleeping");
                    }
                }
            }
        }

        let _ = terminal.draw(|f| {
            draw_ui(f, fd);
        })?;

        trace::debug!("ui drawn");

        trace::debug!("looped");
        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    trace::debug!("ui loop exited");

    disable_raw_mode()?;
    execute!(
        std::io::stdout(),
        Clear(ClearType::Purge),
        LeaveAlternateScreen
    )?;

    trace::debug!("stdout cleanup done");

    trace::debug!("exiting ui loop");
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Compute {
    Next,
    Prev,
}

fn compute_active(
    frame_data: &FrameData,
    compute: Compute,
) -> (Option<String>, usize) {
    let order_len = frame_data.order.len();

    if order_len == 0 {
        return (None, 0);
    }

    let proposed_index: i32 = match compute {
        Compute::Next => frame_data.active_index as i32 + 1,
        Compute::Prev => frame_data.active_index as i32 - 1,
    };

    let active_index = if proposed_index < 0 {
        order_len - 1
    } else {
        proposed_index as usize % order_len
    };

    (frame_data.order.get(active_index).cloned(), active_index)
}

struct FrameData {
    active_index: usize,
    order: Vec<String>,
    paragraph: Paragraph<'static>,
    line_count: usize,
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

fn get_frame_data(buffers: &Screens, active_id: &ActiveId) -> FrameData {
    let order = buffers.lock().keys().rev().cloned().collect::<Vec<_>>();

    let active_id = active_id.lock();
    let active_index = get_active_index(&order, active_id.as_deref());

    let mut buffers = buffers.lock();
    let active_screen = if let Some(id) = order.get(active_index)
        && let Some(buf) = buffers.get_mut(id)
    {
        Some(buf)
    } else {
        None
    };

    let (paragraph, line_count) = if let Some(active_screen) = active_screen {
        active_screen.apply_pending_actions();

        let (paragraph, line_count) = active_screen.paragraph();

        (
            paragraph.block(
                Block::new()
                    .title(format!(
                        "Output - {} ({})",
                        active_screen.title, active_screen.status
                    ))
                    .borders(Borders::ALL),
            ),
            line_count,
        )
    } else {
        (Paragraph::new("<no data>"), 1)
    };

    FrameData {
        active_index,
        order,
        paragraph,
        line_count,
    }
}

/// Draw UI frame: left vertical tab list + right content for selected stream
fn draw_ui<'a>(
    f: &mut Frame<'a>,
    FrameData {
        active_index,
        order,
        paragraph,
        line_count,
    }: FrameData,
) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(10)].as_ref())
        .split(area);

    let items: Vec<ListItem> =
        order.iter().map(|s| ListItem::new(Text::raw(s))).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Tasks"))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol(">> ");

    let order_len = order.len();
    f.render_stateful_widget(list, chunks[0], &mut {
        let mut state = ratatui::widgets::ListState::default();
        if !order.is_empty() {
            state.select(Some(active_index.min(order_len - 1)));
        }
        state
    });

    // right pane: content
    let vp_height = chunks[1].height.saturating_sub(2);

    let scroll_y = (line_count as u16).saturating_sub(vp_height);

    f.render_widget(paragraph.scroll((scroll_y, 0)), chunks[1]);
}

#[derive(Debug, thiserror::Error)]
#[error("tui presenter error: {inner}")]
pub struct TuiPresenterError {
    kind: TuiPresenterErrorKind,
    #[source]
    inner: TuiPresenterErrorInner,
}

impl<T: Into<TuiPresenterErrorInner>> From<T> for TuiPresenterError {
    fn from(e: T) -> Self {
        let inner = e.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(TuiPresenterErrorKind))]
enum TuiPresenterErrorInner {
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
