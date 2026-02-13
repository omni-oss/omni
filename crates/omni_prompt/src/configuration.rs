use crate::{
    PromptExtras,
    parsers::{either_value_or_tera_expr, either_value_or_tera_expr_optional},
};
use derive_new::new;
use either::Either;
use garde::Validate;
use omni_serde_validators::{
    name::validate_name, tera_expr::option_validate_tera_expr,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

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
    #[schemars(with = "Option<EitherUntaggedDef<bool, String>>")]
    #[serde(
        rename = "if",
        with = "either_value_or_tera_expr_optional",
        default
    )]
    pub r#if: Option<Either<bool, String>>,
}

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[garde(allow_unvalidated)]
pub struct ValidateConfiguration {
    /// Accepts a tera expression that will be evaluated to a boolean that determines if the prompt is valid.
    ///
    /// Available Context
    /// - `value`: The value of the prompt.
    #[schemars(with = "EitherUntaggedDef<bool, String>")]
    #[new(into)]
    #[serde(with = "either_value_or_tera_expr")]
    pub condition: Either<bool, String>,

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
pub struct ConfirmPromptConfiguration {
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
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Validate,
    new,
    EnumDiscriminants,
)]
#[strum_discriminants(name(PromptType), vis(pub))]
#[garde(allow_unvalidated)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum PromptConfiguration<TExtra: Default = ()> {
    Confirm {
        #[serde(flatten)]
        #[new(into)]
        prompt: ConfirmPromptConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Select {
        #[serde(flatten)]
        #[new(into)]
        prompt: SelectPromptConfiguration,

        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    MultiSelect {
        #[serde(flatten)]
        #[new(into)]
        prompt: MultiSelectPromptConfiguration,

        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Text {
        #[serde(flatten)]
        #[new(into)]
        prompt: TextPromptConfiguration,

        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Password {
        #[serde(flatten)]
        #[new(into)]
        prompt: PasswordPromptConfiguration,

        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Float {
        #[serde(flatten)]
        #[new(into)]
        prompt: FloatNumberPromptConfiguration,

        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Integer {
        #[serde(flatten)]
        #[new(into)]
        prompt: IntegerNumberPromptConfiguration,

        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
}

impl<TExtra: PromptExtras> PromptConfiguration<TExtra> {
    pub fn extra(&self) -> &TExtra {
        match self {
            PromptConfiguration::Confirm { extra, .. } => extra,
            PromptConfiguration::Select { extra, .. } => extra,
            PromptConfiguration::MultiSelect { extra, .. } => extra,
            PromptConfiguration::Text { extra, .. } => extra,
            PromptConfiguration::Password { extra, .. } => extra,
            PromptConfiguration::Float { extra, .. } => extra,
            PromptConfiguration::Integer { extra, .. } => extra,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            PromptConfiguration::Confirm { prompt, .. } => &prompt.base.name,
            PromptConfiguration::Select { prompt, .. } => &prompt.base.name,
            PromptConfiguration::MultiSelect { prompt, .. } => {
                &prompt.base.base.name
            }
            PromptConfiguration::Text { prompt, .. } => &prompt.base.base.name,
            PromptConfiguration::Password { prompt, .. } => {
                &prompt.base.base.name
            }
            PromptConfiguration::Float { prompt, .. } => &prompt.base.base.name,
            PromptConfiguration::Integer { prompt, .. } => {
                &prompt.base.base.name
            }
        }
    }
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
