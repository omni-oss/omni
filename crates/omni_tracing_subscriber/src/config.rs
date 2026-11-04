use std::path::PathBuf;

use crate::Level;

/// Where developer traces go (RFC 0023, 4.6).
///
/// There is no stdout sink: stdout belongs to command results, and traces
/// there would corrupt piped output and the MCP stdio transport. Users who
/// want traces in a stream use `2>&1` or `--trace-file /dev/stdout`.
#[derive(Debug, Clone, Eq, PartialEq, Default)]
pub struct TracingConfig {
    /// `EnvFilter` directives for stderr (`omni_cache=debug`). `None` is off.
    pub stderr_filter: Option<String>,
    pub file_level: Level,
    pub file_path: Option<PathBuf>,
}

impl TracingConfig {
    /// Returns a config where every sink is off. Suitable for library
    /// consumers that manage tracing externally.
    pub fn disabled() -> Self {
        Self {
            stderr_filter: None,
            file_level: Level::Off,
            file_path: None,
        }
    }
}
