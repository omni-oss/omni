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
    pub name: String,
    pub description: Option<String>,

    #[serde(default)]
    pub prompts: Vec<PromptConfiguration>,

    #[serde(default)]
    pub actions: Vec<ActionConfiguration>,
}
