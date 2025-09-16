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
pub enum TraceLevel {
    #[strum(serialize = "none")]
    None = 0,
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

impl From<TraceLevel> for LevelFilter {
    fn from(value: TraceLevel) -> Self {
        match value {
            TraceLevel::None => LevelFilter::OFF,
            TraceLevel::Error => LevelFilter::ERROR,
            TraceLevel::Warn => LevelFilter::WARN,
            TraceLevel::Info => LevelFilter::INFO,
            TraceLevel::Debug => LevelFilter::DEBUG,
            TraceLevel::Trace => LevelFilter::TRACE,
        }
    }
}
