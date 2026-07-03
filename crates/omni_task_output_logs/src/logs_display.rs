use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{EnumIs, VariantArray};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    VariantArray,
    Serialize,
    Deserialize,
    Default,
    JsonSchema,
    PartialOrd,
    Ord,
    EnumIs,
)]
#[serde(rename_all = "kebab-case")]
pub enum LogsDisplay {
    #[strum(serialize = "all")]
    All,
    #[default]
    #[strum(serialize = "failed")]
    Failed,
    #[strum(serialize = "never")]
    Never,
}

impl LogsDisplay {
    /// Decides whether a task's output should be surfaced given whether the
    /// task failed.
    pub fn should_show(self, failed: bool) -> bool {
        match self {
            LogsDisplay::All => true,
            LogsDisplay::Failed => failed,
            LogsDisplay::Never => false,
        }
    }
}

pub(crate) fn logs_display_from_str<E>(s: &str) -> Result<LogsDisplay, E>
where
    E: serde::de::Error,
{
    LogsDisplay::from_str(s)
        .map_err(|_| E::unknown_variant(s, &["all", "failed", "never"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_failed() {
        assert_eq!(LogsDisplay::default(), LogsDisplay::Failed);
    }

    #[test]
    fn serde_round_trip() {
        for variant in LogsDisplay::VARIANTS {
            let json = serde_json::to_string(variant).unwrap();
            let parsed: LogsDisplay = serde_json::from_str(&json).unwrap();
            assert_eq!(*variant, parsed);
        }
    }

    #[test]
    fn serde_kebab_values() {
        assert_eq!(
            serde_json::to_string(&LogsDisplay::All).unwrap(),
            "\"all\""
        );
        assert_eq!(
            serde_json::to_string(&LogsDisplay::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&LogsDisplay::Never).unwrap(),
            "\"never\""
        );
    }

    #[test]
    fn enum_string_parse() {
        assert_eq!(LogsDisplay::from_str("all").unwrap(), LogsDisplay::All);
        assert_eq!(
            LogsDisplay::from_str("failed").unwrap(),
            LogsDisplay::Failed
        );
        assert_eq!(LogsDisplay::from_str("never").unwrap(), LogsDisplay::Never);
        assert!(LogsDisplay::from_str("nope").is_err());
    }

    #[test]
    fn should_show_truth_table() {
        assert!(LogsDisplay::All.should_show(false));
        assert!(LogsDisplay::All.should_show(true));

        assert!(!LogsDisplay::Failed.should_show(false));
        assert!(LogsDisplay::Failed.should_show(true));

        assert!(!LogsDisplay::Never.should_show(false));
        assert!(!LogsDisplay::Never.should_show(true));
    }
}
