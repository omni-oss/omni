use garde::Validate;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct BasePromptConfiguration<TDefault> {
    pub name: String,
    pub message: String,
    pub default: Option<TDefault>,
    pub hint: Option<String>,
    pub validate: Option<String>,
    #[serde(rename = "if")]
    pub r#if: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct ValidatedPromptConfiguration<TDefault> {
    #[serde(flatten)]
    pub base: BasePromptConfiguration<TDefault>,
    pub validate: Option<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct CheckboxPromptConfiguration {
    #[serde(flatten)]
    pub base: BasePromptConfiguration<bool>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct SelectPromptConfiguration {
    #[serde(flatten)]
    pub base: BasePromptConfiguration<String>,
    pub options: Vec<OptionConfiguration>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct MultiSelectPromptConfiguration {
    #[serde(flatten)]
    pub base: BasePromptConfiguration<Vec<String>>,
    pub options: Vec<OptionConfiguration>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct TextPromptConfiguration {
    #[serde(flatten)]
    pub base: ValidatedPromptConfiguration<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct PasswordPromptConfiguration {
    #[serde(flatten)]
    pub base: ValidatedPromptConfiguration<String>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct NumberPromptConfiguration {
    #[serde(flatten)]
    pub base: ValidatedPromptConfiguration<f64>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum PromptConfiguration {
    Checkbox {
        #[serde(flatten)]
        prompt: CheckboxPromptConfiguration,
    },
    Select {
        #[serde(flatten)]
        prompt: SelectPromptConfiguration,
    },
    MultiSelect {
        #[serde(flatten)]
        prompt: MultiSelectPromptConfiguration,
    },
    Text {
        #[serde(flatten)]
        prompt: TextPromptConfiguration,
    },
    Password {
        #[serde(flatten)]
        prompt: PasswordPromptConfiguration,
    },
    Number {
        #[serde(flatten)]
        prompt: NumberPromptConfiguration,
    },
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[garde(allow_unvalidated)]
pub struct OptionConfiguration {
    pub name: String,
    pub value: String,
}
