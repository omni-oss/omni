use bytes::Bytes;
use derive_new::new;
use strum::{Display, EnumDiscriminants, EnumIs};
use vte::Perform;

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
    pub parser: vte::Parser,
    #[new(default)]
    pub buffer: TaskScreenBuffer,
}

#[derive(new, Default, Clone, PartialEq, Eq)]
pub struct TaskScreenBuffer {
    #[new(default)]
    pub rows: Vec<String>,
    #[new(default)]
    pub current_row: String,
}

#[derive(Debug, Clone, EnumDiscriminants)]
#[strum_discriminants(name(ScreenActionsKind), vis(pub))]
pub enum ScreenAction {
    UpdateStatus(TaskScreenStatus),
    Write(Bytes),
}

impl TaskScreen {
    pub fn buffer(&self) -> String {
        self.buffer.rows.join("\n")
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
                self.parser.advance(&mut self.buffer, &bytes);
            }
        }
    }
}

impl Perform for TaskScreenBuffer {
    fn print(&mut self, c: char) {
        self.current_row.push(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => {
                self.rows.push(std::mem::take(&mut self.current_row));
            }
            _ => {}
        }
    }
}
