use std::path::PathBuf;

use garde::Validate;
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
    pub prompts: Vec<PromptConfiguration>,

    /// Actions to perform
    #[serde(default)]
    pub actions: Vec<ActionConfiguration>,
}
