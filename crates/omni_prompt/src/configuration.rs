use crate::parsers::either_value_or_tera_expr_optional;
use derive_new::new;
use either::Either;
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
    /// Accepts a tera expression that will be evaluated to a boolean that determines if the prompt is valid.
    ///
    /// Available Context
    /// - `value`: The value of the prompt.
    #[new(into)]
    #[serde(deserialize_with = "validate_tera_expr")]
    pub condition: String,

    /// Accepts a tera expression that will be evaluated to a string that will be used as an error message if the prompt is invalid.
    ///
    /// Available Context
    /// - `value`: The value of the prompt.
    #[new(into)]
    #[serde(deserialize_with = "option_validate_tera_expr")]
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

    /// The validation rules for the prompt.
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

    /// The default value of the checkbox.
    /// Accepts a tera expression that will be evaluated to a boolean that will be used as the default value.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[new(into)]
    #[serde(default)]
    #[schemars(with = "Option<EitherUntaggedDef<bool, String>>")]
    #[serde(with = "either_value_or_tera_expr_optional")]
    pub default: Option<Either<bool, String>>,
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

    /// The available options for the select.
    #[new(into)]
    pub options: Vec<OptionConfiguration>,

    /// The default value of the select.
    /// Accepts a tera expression that will be evaluated to a string that will be used as the default value.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
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

    /// The available options for the multi-select.
    #[new(into)]
    pub options: Vec<OptionConfiguration>,

    /// The default value of the multi-select.
    /// Accepts a list of tera expressions that will be evaluated to a list of strings that will be used as the default value.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
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

    /// Accepts a tera expression that will be evaluated to a string that will be used as the default value.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
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

    /// Accepts a tera expression that will be evaluated to a float that will be used as the default value.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[new(into)]
    #[serde(default)]
    #[schemars(with = "Option<EitherUntaggedDef<f64, String>>")]
    #[serde(with = "either_value_or_tera_expr_optional")]
    pub default: Option<Either<f64, String>>,
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

    /// Accepts a tera expression that will be evaluated to an integer that will be used as the default value.
    ///
    /// Available Context
    /// - `prompts`: A dictionary containing the values of the prompts that were asked previously.
    /// - `env`: A dictionary containing the environment variables available for the output directory.
    #[new(into)]
    #[serde(default)]
    #[schemars(with = "Option<EitherUntaggedDef<i64, String>>")]
    #[serde(with = "either_value_or_tera_expr_optional")]
    pub default: Option<Either<i64, String>>,
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
    /// The name of the option.
    #[new(into)]
    pub name: String,

    /// The description of the option.
    #[new(into)]
    #[serde(default)]
    pub description: Option<String>,

    /// The value of the option.
    #[new(into)]
    pub value: String,

    /// Whether to use this option as a separator.
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

#[derive(
    Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[schemars(untagged)]
#[serde(untagged)]
#[garde(allow_unvalidated)]
enum EitherUntaggedDef<L, R> {
    Left(L),
    Right(R),
}
