use bytes::Bytes;
use derive_new::new;
use ratatui::widgets::Paragraph;
use strum::{Display, EnumDiscriminants, EnumIs};

use crate::mux_output_presenter::ansi_parser::AnsiParser;

#[derive(Debug, Clone, Copy, EnumIs, Default, Display)]
pub enum TaskScreenStatus {
    #[default]
    #[strum(serialize = "idle")]
    Idle,
    #[strum(serialize = "in-progress")]
    InProgress,
    #[strum(serialize = "complete")]
    Complete,
}

#[derive(new)]
pub struct TaskScreen {
    pub title: String,
    #[new(default)]
    pub status: TaskScreenStatus,
    pub actions: crossbeam_channel::Receiver<ScreenAction>,
    #[new(default)]
    pub parser: AnsiParser,
}

#[derive(Debug, Clone, EnumDiscriminants)]
#[strum_discriminants(name(ScreenActionsKind), vis(pub))]
pub enum ScreenAction {
    UpdateStatus(TaskScreenStatus),
    Write(Bytes),
}

impl TaskScreen {
    pub fn paragraph(&mut self) -> Paragraph<'static> {
        let snapshot = self.parser.snapshot();
        Paragraph::new(snapshot)
    }

    pub fn apply_pending_actions(&mut self) {
        while let Ok(action) = self.actions.try_recv() {
            self.apply_action(action);
        }
    }

    fn apply_action(&mut self, action: ScreenAction) {
        match action {
            ScreenAction::UpdateStatus(status) => {
                self.status = status;
            }
            ScreenAction::Write(bytes) => {
                self.parser.feed(&bytes);
            }
        }
    }
}
