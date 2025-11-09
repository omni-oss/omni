use std::path::PathBuf;

use garde::Validate;
use omni_serde_validators::tera_expr::{
    option_validate_tera_expr, validate_tera_expr,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumDiscriminants, EnumIs};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct BaseActionConfiguration {
    /// Accepts a tera template that should evaluate to boolean that determines if the action should be executed.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(flatten, deserialize_with = "option_validate_tera_expr")]
    pub r#if: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a name for the action.
    /// Should be unique within the same action group.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(flatten, deserialize_with = "option_validate_tera_expr")]
    pub name: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a progress message.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(flatten, deserialize_with = "option_validate_tera_expr")]
    pub in_progress_message: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a success message.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(flatten, deserialize_with = "option_validate_tera_expr")]
    pub success_message: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a failure message.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    /// - `error`: A string containing the error message that was returned by the action.
    #[serde(flatten, deserialize_with = "option_validate_tera_expr")]
    pub error_message: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct CommonAddConfiguration {
    /// How to handle overwriting existing files.
    pub overwrite: Option<OverwriteConfiguration>,

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

    /// Provide a single template file to add, does not support glob patterns.
    pub template_file: PathBuf,

    /// If provided, it will be stripped from the file names of the template files.
    /// If absent, use the generator's directory as the base path.
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

    /// The path of the file to write to. Will be resolved relative to the output directory.
    pub output_path: PathBuf,
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
#[strum_discriminants(vis(pub), name(ActionConfigurationType), derive(Display))]
pub enum ActionConfiguration {
    #[strum_discriminants(strum(serialize = "add"))]
    Add {
        #[serde(flatten)]
        action: AddActionConfiguration,
    },
    #[strum_discriminants(strum(serialize = "add-inline"))]
    AddInline {
        #[serde(flatten)]
        action: AddInlineActionConfiguration,
    },
    #[strum_discriminants(strum(serialize = "add-many"))]
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
    EnumIs,
    Copy,
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
