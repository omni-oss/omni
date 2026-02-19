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
pub enum Ui {
    #[strum(serialize = "stream")]
    Stream,
    #[strum(serialize = "tui")]
    Tui,
    /// Automatically selects between `stream` and `tui` if there is an interactive
    /// task in the execution plan.
    #[default]
    #[strum(serialize = "auto")]
    Auto,
}
