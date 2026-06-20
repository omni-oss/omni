use std::path::PathBuf;

use crate::Level;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct TracingConfig {
    pub stdout_level: Level,
    pub stdout_show_traces: bool,
    pub file_level: Level,
    pub file_path: Option<PathBuf>,
    pub stderr_level: Level,
    pub stderr_show_traces: bool,
}

impl TracingConfig {
    /// Returns a config where every level is off and no file sink is used.
    /// Suitable for library consumers that manage tracing externally or do not
    /// want any file-based tracing output.
    pub fn disabled() -> Self {
        Self {
            stdout_level: Level::Off,
            stdout_show_traces: false,
            file_level: Level::Off,
            file_path: None,
            stderr_level: Level::Off,
            stderr_show_traces: false,
        }
    }
}
