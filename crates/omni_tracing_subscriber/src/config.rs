use std::path::PathBuf;

use crate::Level;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct TracingConfig {
    pub stdout_level: Level,
    pub stdout_show_traces: bool,
    pub file_level: Level,
    pub file_path: Option<PathBuf>,
    pub stderr_enabled: bool,
    pub stderr_show_traces: bool,
}
