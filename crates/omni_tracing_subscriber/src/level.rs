use strum::{Display, EnumIs, FromRepr, VariantArray};
use tracing_core::LevelFilter;

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    FromRepr,
    EnumIs,
    Display,
    VariantArray,
)]
#[repr(u8)]
pub enum Level {
    #[strum(serialize = "off")]
    Off = 0,
    #[strum(serialize = "error")]
    Error = 1,
    #[strum(serialize = "warn")]
    Warn = 2,
    #[strum(serialize = "info")]
    #[default]
    Info = 3,
    #[strum(serialize = "debug")]
    Debug = 4,
    #[strum(serialize = "trace")]
    Trace = 5,
}

impl From<Level> for LevelFilter {
    fn from(value: Level) -> Self {
        match value {
            Level::Off => LevelFilter::OFF,
            Level::Error => LevelFilter::ERROR,
            Level::Warn => LevelFilter::WARN,
            Level::Info => LevelFilter::INFO,
            Level::Debug => LevelFilter::DEBUG,
            Level::Trace => LevelFilter::TRACE,
        }
    }
}
