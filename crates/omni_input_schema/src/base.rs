use omni_config_types::MaybeExpr;
use omni_serde_validators::name::validate_name;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// A single validator expression applied to an input value.
///
/// `condition` is evaluated as a boolean — `true` means the value is valid.
/// `error_message` is rendered as a Tera template when the condition is false;
/// if absent a default message is generated.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq, Eq, Hash,
)]
#[serde(deny_unknown_fields)]
pub struct ValidateConfiguration {
    /// Tera expression evaluated to a boolean; `true` = value is valid.
    ///
    /// Available context: `value` — the candidate value.
    pub condition: MaybeExpr<bool>,

    /// Tera expression rendered as the error message when `condition` is false.
    ///
    /// Available context: `value` — the candidate value.
    #[serde(
        default,
        deserialize_with = "omni_serde_validators::tera_expr::option_validate_tera_expr"
    )]
    pub error_message: Option<String>,
}

/// The presentation-free fields shared by every `Input<E>` variant.
///
/// `message` is deliberately absent — it is presentation and lives in `E::Base`.
///
/// Deserialization is implemented manually (not derived) so that
/// `serde_path_to_error` can track field paths through the
/// `#[serde(flatten)]` boundary used by all `*Input<E>` variant structs.
/// The flatten machinery passes a `FlatMapDeserializer` here; wrapping it
/// with `serde_path_to_error` before dispatching to the inner struct
/// ensures errors in e.g. `validators[0].condition` carry the full path.
#[derive(Serialize, JsonSchema, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BaseInput {
    /// Unique key used in the resolved-value map.  Must match `[a-zA-Z_][a-zA-Z0-9_]*`.
    pub name: String,

    pub r#if: Option<MaybeExpr<bool>>,

    /// Zero or more Tera validator expressions run against the resolved value.
    pub validators: Vec<ValidateConfiguration>,

    /// When `true` the value must not be logged, echoed, cached, or persisted.
    /// Maps to `writeOnly: true` in the JSON Schema projection.
    pub secret: bool,

    /// Machine-facing help text; emitted as `description` in JSON Schema.
    pub description: Option<String>,
}

impl<'de> Deserialize<'de> for BaseInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        /// Mirror of `BaseInput` with all field-level serde attributes preserved.
        /// Has no `#[serde(flatten)]` fields itself, so `serde_path_to_error`'s
        /// `PathMapAccess` stays live through every key access.
        #[derive(Deserialize)]
        struct Inner {
            #[serde(deserialize_with = "validate_name")]
            name: String,
            r#if: Option<MaybeExpr<bool>>,
            #[serde(default)]
            validators: Vec<ValidateConfiguration>,
            #[serde(default)]
            secret: bool,
            description: Option<String>,
        }

        serde_path_to_error::deserialize(deserializer)
            .map(|i: Inner| BaseInput {
                name: i.name,
                r#if: i.r#if,
                validators: i.validators,
                secret: i.secret,
                description: i.description,
            })
            .map_err(serde::de::Error::custom)
    }
}

impl From<&str> for ValidateConfiguration {
    fn from(condition: &str) -> Self {
        ValidateConfiguration {
            condition: MaybeExpr::new_expr(condition),
            error_message: None,
        }
    }
}

impl From<String> for ValidateConfiguration {
    fn from(condition: String) -> Self {
        ValidateConfiguration {
            condition: MaybeExpr::new_expr(condition),
            error_message: None,
        }
    }
}

impl From<(&str, &str)> for ValidateConfiguration {
    fn from((condition, error_message): (&str, &str)) -> Self {
        ValidateConfiguration {
            condition: MaybeExpr::new_expr(condition),
            error_message: Some(error_message.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_identifier_passes() {
        let b: BaseInput =
            serde_json::from_str(r#"{"name":"my_var"}"#).unwrap();
        assert_eq!(b.name, "my_var");
        assert_eq!(b.r#if, None);
        assert!(!b.secret);
        assert_eq!(b.description, None);
    }

    #[test]
    fn leading_digit_name_fails() {
        let result: Result<BaseInput, _> =
            serde_json::from_str(r#"{"name":"1bad"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn hyphen_in_name_fails() {
        let result: Result<BaseInput, _> =
            serde_json::from_str(r#"{"name":"my-var"}"#);
        assert!(result.is_err());
    }
}
