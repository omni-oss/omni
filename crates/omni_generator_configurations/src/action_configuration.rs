use std::path::PathBuf;

use crate::{
    OmniPath,
    validators::{
        validate_regex, validate_umap_serde_json, validate_umap_target_path,
    },
};
use derive_new::new;
use garde::Validate;
use maps::{Map, UnorderedMap};
use omni_serde_validators::tera_expr::{
    option_validate_tera_expr, validate_tera_expr,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{
    Display, EnumCount, EnumDiscriminants, EnumIs, EnumIter, EnumString,
    VariantArray,
};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct BaseActionConfiguration {
    /// Accepts a tera template that should evaluate to boolean that determines if the action should be executed.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(default, deserialize_with = "option_validate_tera_expr")]
    pub r#if: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a name for the action.
    /// Should be unique within the same action group.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(default, deserialize_with = "option_validate_tera_expr")]
    pub name: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a progress message.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(default, deserialize_with = "option_validate_tera_expr")]
    pub in_progress_message: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a success message.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[serde(default, deserialize_with = "option_validate_tera_expr")]
    pub success_message: Option<String>,

    /// Accepts a tera template that should evaluate to a string that will be used as a failure message.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    /// - `error`: A string containing the error message that was returned by the action.
    #[serde(default, deserialize_with = "option_validate_tera_expr")]
    pub error_message: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct CommonAddConfiguration {
    /// How to handle overwriting existing files.
    #[serde(default)]
    pub overwrite: Option<OverwriteConfiguration>,

    #[serde(default)]
    pub target: Option<String>,

    #[serde(default, deserialize_with = "validate_umap_serde_json")]
    pub data: UnorderedMap<String, serde_json::Value>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct BaseAddActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(flatten, default)]
    pub common: CommonAddConfiguration,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct AddActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,

    /// Provide a single template file to add, does not support glob patterns.
    #[new(into)]
    pub template_file: PathBuf,

    /// If provided, it will be stripped from the file names of the template files.
    /// If absent, use the generator's directory as the base path.
    #[new(into)]
    #[serde(default)]
    pub base_path: Option<PathBuf>,

    /// Disregard the folder structure of the template files and flatten them into write them into a single directory.
    #[new(into)]
    #[serde(default)]
    pub flatten: bool,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
/// Use an single inline template and write it to a file.
pub struct AddContentActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,

    /// Accepts an inline tera template that will be evaluated to a string that will be used to produce the file.
    #[new(into)]
    #[serde(deserialize_with = "validate_tera_expr")]
    pub template: String,

    /// The path of the file to write to. Will be resolved relative to the output directory.
    #[new(into)]
    pub output_path: PathBuf,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct AddManyActionConfiguration {
    #[serde(flatten)]
    pub base: BaseAddActionConfiguration,

    /// Provide a list of template files to add, accepts glob patterns.
    pub template_files: Vec<PathBuf>,

    /// Disregard the folder structure of the template files and flatten them into write them into a single directory.
    #[serde(default)]
    pub flatten: bool,

    /// If provided, it will be stripped from the file names of the template files.
    /// If absent, use the generator's directory as the base path.
    #[serde(default)]
    pub base_path: Option<PathBuf>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct RunGeneratorActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(alias = "gen")]
    pub generator: String,

    #[serde(default)]
    pub prompt_values: PromptValuesConfiguration,

    /// Target directories to place the generated files.
    /// Overrides the targets in the generator configuration.
    #[serde(deserialize_with = "validate_umap_target_path")]
    #[serde(default)]
    pub targets: UnorderedMap<String, OmniPath>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct CommonModifyConfiguration {
    pub target: String,

    #[serde(deserialize_with = "validate_regex")]
    pub pattern: String,

    #[serde(default, deserialize_with = "validate_umap_serde_json")]
    pub data: UnorderedMap<String, serde_json::Value>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct ModifyActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(flatten)]
    pub common: CommonModifyConfiguration,

    pub template_file: PathBuf,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct ModifyContentActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(flatten)]
    pub common: CommonModifyConfiguration,

    #[serde(deserialize_with = "validate_tera_expr")]
    pub template: String,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct CommonInsertConfiguration {
    #[serde(flatten)]
    pub common: CommonModifyConfiguration,

    #[serde(default = "default_separator")]
    pub separator: String,

    #[serde(default = "default_unique")]
    pub unique: bool,
}

fn default_separator() -> String {
    "\n".to_string()
}

fn default_unique() -> bool {
    true
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct AppendActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(flatten)]
    pub common: CommonInsertConfiguration,

    pub template_file: PathBuf,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct AppendContentActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(flatten)]
    pub common: CommonInsertConfiguration,

    #[serde(deserialize_with = "validate_tera_expr")]
    pub template: String,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct PrependActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(flatten)]
    pub common: CommonInsertConfiguration,

    pub template_file: PathBuf,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct PrependContentActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    #[serde(flatten)]
    pub common: CommonInsertConfiguration,

    #[serde(deserialize_with = "validate_tera_expr")]
    pub template: String,
}

fn default_true() -> bool {
    true
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct CommonRunCustomActionConfiguration {
    /// By default, running this type of action is not supported in dry run mode.
    /// If set to true, this action will be run even in dry run mode.
    #[serde(default)]
    pub supports_dry_run: bool,

    /// Will be used as the working directory for the action execution.
    #[serde(default)]
    pub target: Option<String>,

    /// Additional environment variables to expose to the script/command.
    #[serde(default)]
    pub env: Map<String, String>,

    /// Show stdout and stderr from the action.
    #[serde(default = "default_true")]
    pub show_output: bool,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct RunCommandActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    /// The command to run.
    #[serde(deserialize_with = "validate_tera_expr")]
    pub command: String,

    #[serde(flatten)]
    pub common: CommonRunCustomActionConfiguration,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct RunJavaScriptActionConfiguration {
    #[serde(flatten)]
    pub base: BaseActionConfiguration,

    /// Arguments to pass to the script.
    #[serde(default, deserialize_with = "validate_umap_serde_json")]
    pub args: UnorderedMap<String, serde_json::Value>,

    /// The path of the file to run.
    pub script: PathBuf,

    #[serde(flatten)]
    pub common: CommonRunCustomActionConfiguration,
}

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Validate,
    new,
    Default,
)]
#[garde(allow_unvalidated)]
pub struct PromptValuesConfiguration {
    #[serde(default)]
    pub forward: ForwardPromptValuesConfiguration,
    #[serde(default)]
    pub values: UnorderedMap<String, PromptValue>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
#[serde(untagged)]
pub enum PromptValue {
    Integer(i64),
    Float(f64),
    Boolean(bool),
    String(String),
    List(Vec<PromptValue>),
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
#[serde(untagged)]
pub enum ForwardPromptValuesConfiguration {
    ForAll(ForAllPromptValuesConfiguration),
    Selected(Vec<String>),
}

impl Default for ForwardPromptValuesConfiguration {
    fn default() -> Self {
        ForwardPromptValuesConfiguration::ForAll(
            ForAllPromptValuesConfiguration::None,
        )
    }
}

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Validate,
    new,
    Default,
)]
#[serde(rename_all = "kebab-case")]
#[garde(allow_unvalidated)]
pub enum ForAllPromptValuesConfiguration {
    All,
    #[default]
    None,
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
    new,
)]
#[garde(allow_unvalidated)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
#[strum_discriminants(vis(pub), name(ActionConfigurationType), derive(Display))]
pub enum ActionConfiguration {
    /// Add a single file specified by a template file
    #[strum_discriminants(strum(serialize = "add"))]
    Add {
        #[serde(flatten)]
        action: AddActionConfiguration,
    },

    /// Add a single file specified by an inline template in the configuration
    #[strum_discriminants(strum(serialize = "add-content"))]
    AddContent {
        #[serde(flatten)]
        action: AddContentActionConfiguration,
    },

    /// Add multiple files specified by a list of template files, accepts glob patterns
    #[strum_discriminants(strum(serialize = "add-many"))]
    AddMany {
        #[serde(flatten)]
        action: AddManyActionConfiguration,
    },

    /// Run a generator
    #[strum_discriminants(strum(serialize = "run-generator"))]
    RunGenerator {
        #[serde(flatten)]
        action: RunGeneratorActionConfiguration,
    },

    /// Replace a text using a tera template
    #[strum_discriminants(strum(serialize = "modify"))]
    Modify {
        #[serde(flatten)]
        action: ModifyActionConfiguration,
    },

    /// Replace a text using a tera template
    #[strum_discriminants(strum(serialize = "modify-content"))]
    ModifyContent {
        #[serde(flatten)]
        action: ModifyContentActionConfiguration,
    },

    /// Append a text rendered from a tera template after a line matching a regex
    #[strum_discriminants(strum(serialize = "append"))]
    Append {
        #[serde(flatten)]
        action: AppendActionConfiguration,
    },

    /// Append a text rendered from a tera template after a line matching a regex
    #[strum_discriminants(strum(serialize = "append-content"))]
    AppendContent {
        #[serde(flatten)]
        action: AppendContentActionConfiguration,
    },

    /// Prepend a text rendered from a tera template after a line matching a regex
    #[strum_discriminants(strum(serialize = "prepend"))]
    Prepend {
        #[serde(flatten)]
        action: PrependActionConfiguration,
    },

    /// Prepend a text rendered from a tera template after a line matching a regex
    #[strum_discriminants(strum(serialize = "prepend-content"))]
    PrependContent {
        #[serde(flatten)]
        action: PrependContentActionConfiguration,
    },

    /// Run a custom command
    #[strum_discriminants(strum(serialize = "run-command"))]
    RunCommand {
        #[serde(flatten)]
        action: RunCommandActionConfiguration,
    },

    /// Run a custom JavaScript program
    #[strum_discriminants(strum(serialize = "run-javascript"))]
    #[serde(rename = "run-javascript")]
    #[serde(skip)]
    RunJavaScript {
        #[serde(flatten)]
        action: RunJavaScriptActionConfiguration,
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
    Display,
    EnumIter,
    EnumString,
    EnumCount,
    VariantArray,
)]
#[serde(rename_all = "kebab-case")]
pub enum OverwriteConfiguration {
    /// Prompt the user to confirm overwriting existing files.
    #[default]
    #[strum(serialize = "prompt")]
    Prompt,

    /// Always overwrite existing files.
    #[strum(serialize = "always")]
    Always,

    /// Never overwrite existing files.
    #[strum(serialize = "never")]
    Never,
}
