use crate::parsers::{
    either_value_or_tera_expr, either_value_or_tera_expr_optional,
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
use value_bag::{OwnedValueBag, ValueBag};

pub trait InputExtras:
    for<'de> Deserialize<'de>
    + Serialize
    + JsonSchema
    + Clone
    + std::fmt::Debug
    + PartialEq
    + Validate
    + Default
{
}

impl<T> InputExtras for T where
    T: for<'de> Deserialize<'de>
        + Serialize
        + JsonSchema
        + Clone
        + std::fmt::Debug
        + PartialEq
        + Validate
        + Default
{
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
pub struct BaseInputConfiguration {
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
    /// Accepts a tera expression that will be evaluated to a boolean that determines if the input is valid.
    ///
    /// Available Context
    /// - `value`: The value of the input.
    #[schemars(with = "EitherUntaggedDef<bool, String>")]
    #[new(into)]
    #[serde(with = "either_value_or_tera_expr")]
    pub condition: Either<bool, String>,

    /// Accepts a tera expression that will be evaluated to a string that will be used as an error message if the input is invalid.
    ///
    /// Available Context
    /// - `value`: The value of the input.
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
pub struct ValidatedInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: BaseInputConfiguration,

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
pub struct ConfirmInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: BaseInputConfiguration,

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
pub struct SelectInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: BaseInputConfiguration,

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
pub struct MultiSelectInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedInputConfiguration,

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
pub struct TextInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedInputConfiguration,

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
pub struct PasswordInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedInputConfiguration,
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
pub struct FloatInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedInputConfiguration,

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
pub struct IntegerInputConfiguration {
    #[serde(flatten)]
    #[new(into)]
    pub base: ValidatedInputConfiguration,

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
#[strum_discriminants(name(InputType), vis(pub))]
#[garde(allow_unvalidated)]
#[serde(rename_all = "kebab-case")]
#[serde(tag = "type")]
pub enum InputConfiguration<TExtra: Default = ()> {
    Confirm {
        #[serde(flatten)]
        #[new(into)]
        input: ConfirmInputConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Select {
        #[serde(flatten)]
        #[new(into)]
        input: SelectInputConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    MultiSelect {
        #[serde(flatten)]
        #[new(into)]
        input: MultiSelectInputConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Text {
        #[serde(flatten)]
        #[new(into)]
        input: TextInputConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Password {
        #[serde(flatten)]
        #[new(into)]
        input: PasswordInputConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Float {
        #[serde(flatten)]
        #[new(into)]
        input: FloatInputConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
    Integer {
        #[serde(flatten)]
        #[new(into)]
        input: IntegerInputConfiguration,
        #[serde(flatten)]
        #[new(default)]
        extra: TExtra,
    },
}

impl<TExtra: InputExtras> InputConfiguration<TExtra> {
    pub fn extra(&self) -> &TExtra {
        match self {
            InputConfiguration::Confirm { extra, .. } => extra,
            InputConfiguration::Select { extra, .. } => extra,
            InputConfiguration::MultiSelect { extra, .. } => extra,
            InputConfiguration::Text { extra, .. } => extra,
            InputConfiguration::Password { extra, .. } => extra,
            InputConfiguration::Float { extra, .. } => extra,
            InputConfiguration::Integer { extra, .. } => extra,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            InputConfiguration::Confirm { input, .. } => &input.base.name,
            InputConfiguration::Select { input, .. } => &input.base.name,
            InputConfiguration::MultiSelect { input, .. } => {
                &input.base.base.name
            }
            InputConfiguration::Text { input, .. } => &input.base.base.name,
            InputConfiguration::Password { input, .. } => &input.base.base.name,
            InputConfiguration::Float { input, .. } => &input.base.base.name,
            InputConfiguration::Integer { input, .. } => &input.base.base.name,
        }
    }

    pub fn default_value(&self) -> Option<OwnedValueBag> {
        Some(match self {
            InputConfiguration::Confirm { input, .. } => {
                unwrap_either_to_vbag(input.default.as_ref()?)
            }
            InputConfiguration::Select { input, .. } => {
                let value = input.default.as_ref()?;
                ValueBag::from_serde1(&value).to_owned()
            }
            InputConfiguration::MultiSelect { input, .. } => {
                let value = input.default.as_ref()?;
                ValueBag::from_serde1(&value).to_owned()
            }
            InputConfiguration::Text { input, .. } => {
                let value = input.default.as_ref()?;
                ValueBag::from_serde1(&value).to_owned()
            }
            InputConfiguration::Password { .. } => {
                return None;
            }
            InputConfiguration::Float { input, .. } => {
                unwrap_either_to_vbag(input.default.as_ref()?)
            }
            InputConfiguration::Integer { input, .. } => {
                unwrap_either_to_vbag(input.default.as_ref()?)
            }
        })
    }
}

fn unwrap_either_to_vbag<L: serde::Serialize, R: serde::Serialize>(
    either: &Either<L, R>,
) -> OwnedValueBag {
    match either {
        Either::Left(l) => ValueBag::from_serde1(l).to_owned(),
        Either::Right(r) => ValueBag::from_serde1(r).to_owned(),
    }
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
pub struct CollectionConfig<'a> {
    /// Variable name under which already-collected values are exposed
    /// when evaluating `if` Tera expressions.
    pub if_expressions_root_property: Option<&'a str>,
    /// Variable name under which the candidate value is exposed
    /// when evaluating validator Tera expressions.
    pub validation_value_name: Option<&'a str>,
    /// When `true`, inputs with a `default` field skip interactive collection.
    pub use_defaults: bool,
}

impl<'a> Default for CollectionConfig<'a> {
    fn default() -> Self {
        Self {
            if_expressions_root_property: Some("inputs"),
            validation_value_name: Some("value"),
            use_defaults: false,
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
