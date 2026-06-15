use std::path::PathBuf;

use crate::{
    OmniPath,
    validators::{validate_umap_serde_json, validate_umap_target_path},
};
use garde::Validate;
use maps::UnorderedMap;
use omni_input_provider::InputConfiguration;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ActionConfiguration;

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct GeneratorConfiguration {
    /// Absolute path to the configuration file where this is serialized from
    #[serde(default)]
    #[serde(skip)]
    pub config_path: PathBuf,

    /// Scope identifier. Once set, only generators with the same scope id will be able to call and be called by this generator.
    #[serde(default)]
    #[serde(skip)]
    pub scope_id: Option<String>,

    /// Unique name of the generator
    pub name: String,

    /// Display name of the generator, if not provided, the name will be used
    #[serde(default)]
    pub display_name: Option<String>,

    /// Description of the generator
    pub description: Option<String>,

    /// Prompts to ask the user
    #[serde(default)]
    #[serde(rename = "prompts")]
    pub inputs: Vec<InputConfiguration<InputConfigurationExtra>>,

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
pub struct InputConfigurationExtra {
    /// Whether to remember the value of this prompt to the session so that future invocations of the generator don't ask the user again
    /// when used in the same directory.
    #[serde(default)]
    pub remember: bool,
}
