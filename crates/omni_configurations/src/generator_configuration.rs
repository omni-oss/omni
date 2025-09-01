use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct GeneratorConfiguration {
    pub name: String,
    #[serde(default)]
    pub prompts: Vec<Prompt>,
    #[serde(default)]
    pub files: Vec<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct BasePrompt<TDefault> {
    pub name: String,
    pub message: String,
    pub default: Option<TDefault>,
    pub hint: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum Prompt {
    Boolean {
        #[serde(flatten)]
        base: BasePrompt<bool>,
    },
    Options {
        #[serde(flatten)]
        base: BasePrompt<String>,
        choices: Vec<String>,
        #[serde(default)]
        multiple: bool,
        #[serde(default, alias = "allow-custom")]
        allow_custom: bool,
    },
    Text {
        #[serde(flatten)]
        base: BasePrompt<String>,
        #[serde(alias = "min-length")]
        min_length: Option<usize>,
        #[serde(alias = "max-length")]
        max_length: Option<usize>,
        pattern: Option<String>,
    },
    Number {
        #[serde(flatten)]
        base: BasePrompt<f64>,
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
}
