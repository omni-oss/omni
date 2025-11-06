use std::path::PathBuf;

use garde::Validate;
use omni_serde_validators::tera_expr::{
    option_validate_tera_expr, validate_tera_expr,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct BaseActionConfiguration {
    /// Accepts a tera expression that will be evaluated to boolean that determines if the action should be executed.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(flatten, deserialize_with = "option_validate_tera_expr")]
    pub r#if: Option<String>,

    /// Accepts a tera expression that will be evaluated to a string that will be used as a progress message.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(flatten, deserialize_with = "option_validate_tera_expr")]
    pub progress_message: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct CommonAddConfiguration {
    /// How to handle overwriting existing files.
    pub overwrite: Option<OverwriteConfiguration>,

    /// Target directory to add the file(s) to. If it does not exist, it will be created.
    pub target: Option<PathBuf>,
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

    /// Provide a single template file to add, does not support glob patterns.
    pub template_file: PathBuf,

    /// If provided, it will be stripped from the file names of the template files.
    #[serde(default)]
    pub base_path: Option<PathBuf>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
/// Use an single inline template and write it to a file.
pub struct AddInlineActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,

    /// Accepts an inline tera template that will be evaluated to a string that will be used to produce the file.
    #[serde(deserialize_with = "validate_tera_expr")]
    pub template: String,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct AddManyActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,

    /// Provide a list of template files to add, accepts glob patterns.
    pub template_files: Vec<PathBuf>,

    /// Disregard the folder structure of the template files and flatten them into write them into a single directory.
    #[serde(default)]
    pub flatten: Option<bool>,

    /// If provided, it will be stripped from the file names of the template files.
    /// If absent, use the generator's directory as the base path.
    #[serde(default)]
    pub base_path: Option<PathBuf>,
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
    /// Prompt the user to confirm overwriting existing files.
    #[default]
    Prompt,

    /// Always overwrite existing files.
    Always,

    /// Never overwrite existing files.
    Never,
}
