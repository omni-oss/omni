use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIs, VariantArray};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    EnumIs,
    Display,
    VariantArray,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Force {
    #[strum(serialize = "all")]
    All,
    #[strum(serialize = "failed")]
    Failed,
    #[default]
    #[strum(serialize = "none")]
    None,
}
