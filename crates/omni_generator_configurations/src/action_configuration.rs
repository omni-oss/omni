use std::path::PathBuf;

use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct BaseActionConfiguration {
    #[serde(flatten)]
    pub r#if: Option<String>,
    pub progress_message: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct CommonAddConfiguration {
    pub overwrite: Option<OverwriteConfiguration>,
    /// Target directory to add the file(s) to. If it does not exist, it will be created.
    pub target: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct BaseAddActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,
    #[serde(flatten, default)]
    pub common: CommonAddConfiguration,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct AddActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,
    pub template_file: PathBuf,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct AddInlineActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,
    pub template: String,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct AddManyActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,
    pub template_files: Vec<PathBuf>,
    pub flatten: Option<bool>,
}

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Validate,
    EnumDiscriminants,
)]
#[garde(allow_unvalidated)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
#[strum_discriminants(vis(pub), name(ActionConfigurationType))]
pub enum ActionConfiguration {
    Add {
        #[serde(flatten)]
        action: AddActionConfiguration,
    },
    AddInline {
        #[serde(flatten)]
        action: AddInlineActionConfiguration,
    },
    AddMany {
        #[serde(flatten)]
        action: AddManyActionConfiguration,
    },
}

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Validate,
    Default,
)]
#[serde(rename_all = "kebab-case")]
pub enum OverwriteConfiguration {
    #[default]
    Prompt,
    Always,
    Never,
}
