use std::path::PathBuf;

use crate::TraceLevel;

#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct TracingConfig {
    pub stdout_trace_level: TraceLevel,
    pub file_trace_level: TraceLevel,
    pub file_path: Option<PathBuf>,
    pub stderr_trace_enabled: bool,
}
