use std::path::PathBuf;

use crate::{
    OmniPath,
    validators::{validate_umap_serde_json, validate_umap_target_path},
};
use garde::Validate;
use maps::UnorderedMap;
use omni_prompt::configuration::PromptConfiguration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ActionConfiguration;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct GeneratorConfiguration {
    /// Unique name of the generator
    pub name: String,

    /// Display name of the generator, if not provided, the name will be used
    #[serde(default)]
    pub display_name: Option<String>,

    /// Description of the generator
    pub description: Option<String>,

    #[serde(default)]
    #[serde(skip)]
    pub file: PathBuf,

    /// Prompts to ask the user
    #[serde(default)]
    pub prompts: Vec<PromptConfiguration<PromptConfigurationExtra>>,

    /// Actions to perform
    #[serde(default)]
    pub actions: Vec<ActionConfiguration>,

    /// Variables to use in the generator, these are evaluated after the prompts
    ///
    /// The variables are available in the templates as `{{ vars.var_name }}`
    ///
    /// Available context variables:
    /// - `prompts`: The values of the prompts
    #[serde(default)]
    #[serde(deserialize_with = "validate_umap_serde_json")]
    pub vars: UnorderedMap<String, serde_json::Value>,

    /// Target directories to place the generated files
    /// Target directories to add the file(s) to. If it does not exist, it will be created.
    #[serde(deserialize_with = "validate_umap_target_path")]
    #[serde(default)]
    pub targets: UnorderedMap<String, OmniPath>,
}

#[derive(
    Default,
    Debug,
    Clone,
    PartialEq,
    Serialize,
    Deserialize,
    JsonSchema,
    Validate,
)]
#[garde(allow_unvalidated)]
pub struct PromptConfigurationExtra {
    /// Whether to remember the value of this prompt to the session so that future invocations of the generator don't ask the user again
    /// when used in the same directory.
    #[serde(default)]
    pub remember: bool,
}
