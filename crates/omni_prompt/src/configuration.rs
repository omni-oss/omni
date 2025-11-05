use derive_new::new;
use garde::Validate;
use omni_serde_validators::{
    name::validate_name,
    tera_expr::{option_validate_tera_expr, validate_tera_expr},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
pub struct BasePromptConfiguration {
    #[new(into)]
    #[serde(deserialize_with = "validate_name")]
    pub name: String,

    #[new(into)]
    pub message: String,

    #[new(into)]
    #[serde(
        rename = "if",
        deserialize_with = "option_validate_tera_expr",
        default
    )]
    pub r#if: Option<String>,
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
pub struct ValidateConfiguration {
    #[new(into)]
    #[serde(deserialize_with = "validate_tera_expr")]
    pub condition: String,

    #[new(into)]
    pub error_message: Option<String>,
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
pub struct ValidatedPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: BasePromptConfiguration,

    #[new(into)]
    #[serde(default)]
    pub validate: Vec<ValidateConfiguration>,
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
pub struct CheckboxPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: BasePromptConfiguration,

    #[new(into)]
    pub default: Option<bool>,
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
pub struct SelectPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: BasePromptConfiguration,

    #[new(into)]
    pub options: Vec<OptionConfiguration>,

    #[new(into)]
    #[serde(default)]
    pub default: Option<String>,
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
pub struct MultiSelectPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedPromptConfiguration,

    #[new(into)]
    pub options: Vec<OptionConfiguration>,

    #[new(into)]
    #[serde(default)]
    pub default: Option<Vec<String>>,
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
pub struct TextPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedPromptConfiguration,

    #[new(into)]
    #[serde(default)]
    pub default: Option<String>,
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
pub struct PasswordPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedPromptConfiguration,
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
pub struct FloatNumberPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedPromptConfiguration,

    #[new(into)]
    #[serde(default)]
    pub default: Option<f64>,
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
pub struct IntegerNumberPromptConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedPromptConfiguration,

    #[new(into)]
    #[serde(default)]
    pub default: Option<i64>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum PromptConfiguration {
    Checkbox {
        #[serde(flatten)]
        #[new(into)]
        prompt: CheckboxPromptConfiguration,
    },
    Select {
        #[serde(flatten)]
        #[new(into)]
        prompt: SelectPromptConfiguration,
    },
    MultiSelect {
        #[serde(flatten)]
        #[new(into)]
        prompt: MultiSelectPromptConfiguration,
    },
    Text {
        #[serde(flatten)]
        #[new(into)]
        prompt: TextPromptConfiguration,
    },
    Password {
        #[serde(flatten)]
        #[new(into)]
        prompt: PasswordPromptConfiguration,
    },
    Float {
        #[serde(flatten)]
        #[new(into)]
        prompt: FloatNumberPromptConfiguration,
    },
    Integer {
        #[serde(flatten)]
        #[new(into)]
        prompt: IntegerNumberPromptConfiguration,
    },
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct OptionConfiguration {
    #[new(into)]
    pub name: String,

    #[new(into)]
    #[serde(default)]
    pub description: Option<String>,

    #[new(into)]
    pub value: String,

    #[new(into)]
    #[serde(default)]
    pub separator: bool,
}

#[derive(Debug, new)]
pub struct PromptingConfiguration<'a> {
    pub if_expressions_root_property: Option<&'a str>,
    pub validation_expressions_value_name: Option<&'a str>,
}

impl<'a> Default for PromptingConfiguration<'a> {
    fn default() -> Self {
        Self {
            if_expressions_root_property: Some("prompts"),
            validation_expressions_value_name: Some("value"),
        }
    }
}
