use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::VariantArray;

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
)]
#[serde(rename_all = "kebab-case")]
pub enum Ui {
    #[default]
    #[strum(serialize = "stream")]
    Stream,
    #[strum(serialize = "tui")]
    Tui,
}
