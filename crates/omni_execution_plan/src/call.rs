use derive_new::new;
use strum::{Display, EnumIs};

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, new, Display, EnumIs,
)]
pub enum Call {
    #[strum(to_string = "command '{command} {args:?}'")]
    Command {
        #[new(into)]
        command: String,
        args: Vec<String>,
    },

    #[strum(to_string = "task '{0}'")]
    Task(#[new(into)] String),
}
