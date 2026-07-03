use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    logs_display::LogsDisplay,
    output_logs_configuration::OutputLogsConfiguration,
};

/// The resolved per-task display policy the subscriber acts on, with both
/// facets collapsed to concrete values.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub struct EffectiveOutputLogs {
    /// Fresh (cache-miss) output.
    pub new: LogsDisplay,
    /// Replayed cached (cache-hit) output.
    pub cached: LogsDisplay,
}

impl EffectiveOutputLogs {
    /// Resolves the effective policy per facet, following the precedence
    /// `flag > task/project (resolved config) > default`. The `cached` facet
    /// falls back to the `--output-logs` flag before the resolved config.
    pub fn resolve(
        flag_new: Option<LogsDisplay>,
        flag_cached: Option<LogsDisplay>,
        resolved: Option<&OutputLogsConfiguration>,
        default: LogsDisplay,
    ) -> Self {
        let (cfg_new, cfg_cached) =
            resolved.map(|c| c.normalized()).unwrap_or((None, None));

        let new = flag_new.or(cfg_new).unwrap_or(default);
        let cached = flag_cached.or(flag_new).or(cfg_cached).unwrap_or(default);

        Self { new, cached }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_logs_configuration::OutputLogsSplit;

    #[test]
    fn default_is_failed_both_facets() {
        let effective = EffectiveOutputLogs::default();
        assert_eq!(effective.new, LogsDisplay::Failed);
        assert_eq!(effective.cached, LogsDisplay::Failed);
    }

    #[test]
    fn falls_back_to_default_when_nothing_set() {
        let effective =
            EffectiveOutputLogs::resolve(None, None, None, LogsDisplay::Failed);
        assert_eq!(effective.new, LogsDisplay::Failed);
        assert_eq!(effective.cached, LogsDisplay::Failed);
    }

    #[test]
    fn uses_resolved_config_when_no_flags() {
        let config = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: Some(LogsDisplay::All),
            cached: Some(LogsDisplay::Never),
        });
        let effective = EffectiveOutputLogs::resolve(
            None,
            None,
            Some(&config),
            LogsDisplay::Failed,
        );
        assert_eq!(effective.new, LogsDisplay::All);
        assert_eq!(effective.cached, LogsDisplay::Never);
    }

    #[test]
    fn flag_new_overrides_config() {
        let config = OutputLogsConfiguration::Uniform(LogsDisplay::Never);
        let effective = EffectiveOutputLogs::resolve(
            Some(LogsDisplay::All),
            None,
            Some(&config),
            LogsDisplay::Failed,
        );
        // flag_new wins for `new`; `cached` falls back to flag_new before config
        assert_eq!(effective.new, LogsDisplay::All);
        assert_eq!(effective.cached, LogsDisplay::All);
    }

    #[test]
    fn flag_cached_overrides_only_cached() {
        let config = OutputLogsConfiguration::Uniform(LogsDisplay::Failed);
        let effective = EffectiveOutputLogs::resolve(
            None,
            Some(LogsDisplay::Never),
            Some(&config),
            LogsDisplay::Failed,
        );
        assert_eq!(effective.new, LogsDisplay::Failed);
        assert_eq!(effective.cached, LogsDisplay::Never);
    }

    #[test]
    fn cached_falls_back_to_flag_new_over_config() {
        let config = OutputLogsConfiguration::Split(OutputLogsSplit {
            new: Some(LogsDisplay::Never),
            cached: Some(LogsDisplay::Never),
        });
        let effective = EffectiveOutputLogs::resolve(
            Some(LogsDisplay::All),
            None,
            Some(&config),
            LogsDisplay::Failed,
        );
        assert_eq!(effective.new, LogsDisplay::All);
        assert_eq!(effective.cached, LogsDisplay::All);
    }

    #[test]
    fn flag_cached_beats_flag_new_for_cached_facet() {
        let effective = EffectiveOutputLogs::resolve(
            Some(LogsDisplay::All),
            Some(LogsDisplay::Never),
            None,
            LogsDisplay::Failed,
        );
        assert_eq!(effective.new, LogsDisplay::All);
        assert_eq!(effective.cached, LogsDisplay::Never);
    }
}
