use serde::{Deserialize, Serialize};

/// Severity of a [`ScriptLogEvent`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::Display,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ScriptLogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// A line a generator script logged (`ctx.log.info(..)`).
///
/// This is the script author's own output, not an omni notice or a trace:
/// presenters show `info`, `warn` and `error`, while `debug` and `trace` go
/// to `tracing` only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptLogEvent {
    pub level: ScriptLogLevel,
    pub target: String,
    pub message: String,
}

impl ScriptLogEvent {
    /// Mirrors the record into `tracing`, so it appears under `--trace`.
    pub fn mirror_to_tracing(&self) {
        let (target, msg) = (self.target.as_str(), self.message.as_str());
        match self.level {
            ScriptLogLevel::Error => {
                tracing::error!(script_target = target, "{msg}")
            }
            ScriptLogLevel::Warn => {
                tracing::warn!(script_target = target, "{msg}")
            }
            ScriptLogLevel::Info => {
                tracing::info!(script_target = target, "{msg}")
            }
            ScriptLogLevel::Debug => {
                tracing::debug!(script_target = target, "{msg}")
            }
            ScriptLogLevel::Trace => {
                tracing::trace!(script_target = target, "{msg}")
            }
        }
    }
}
