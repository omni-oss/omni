use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::utils::default_true;

/// # Ignore Configuration
/// Controls `omni ignore sync`: which files carry omni's managed block and which
/// pattern sources contribute to it.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct IgnoreConfig {
    /// Files to patch. Absent or empty resolves to `.gitignore`, `.ignore`, and
    /// `.omniignore`, all at the workspace root.
    #[serde(default)]
    pub files: Vec<String>,

    #[serde(default)]
    pub sources: IgnoreSourcesConfig,
}

/// Toggles for the pattern sources that feed the managed block. Both default on.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct IgnoreSourcesConfig {
    /// Emit patterns for omni's own `.omni/` state.
    #[serde(default = "default_true")]
    pub internal: bool,

    /// Emit an anchored pattern for every projection link in the ledger.
    #[serde(default = "default_true")]
    pub projections: bool,
}

impl Default for IgnoreSourcesConfig {
    fn default() -> Self {
        Self {
            internal: true,
            projections: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_sources_default_both_on() {
        let cfg: IgnoreConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.files, Vec::<String>::new());
        assert!(cfg.sources.internal);
        assert!(cfg.sources.projections);
    }

    #[test]
    fn a_single_source_toggle_leaves_the_other_on() {
        let cfg: IgnoreConfig =
            serde_json::from_str(r#"{"sources":{"projections":false}}"#)
                .unwrap();
        assert!(cfg.sources.internal);
        assert!(!cfg.sources.projections);
    }

    #[test]
    fn round_trips_through_json() {
        let cfg = IgnoreConfig {
            files: vec![".gitignore".to_string()],
            sources: IgnoreSourcesConfig {
                internal: false,
                projections: true,
            },
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: IgnoreConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn default_matches_the_full_enabled_shape() {
        let cfg = IgnoreConfig::default();
        assert!(cfg.files.is_empty());
        assert!(cfg.sources.internal);
        assert!(cfg.sources.projections);
    }
}
