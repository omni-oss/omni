use std::borrow::Cow;

use enumset::{EnumSet, enum_set};
use omni_input_schema::{
    FloatArrayInput, InputKind, InputProfile, IntegerArrayInput,
    StringArrayInput, StringInput,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Presentation marker for generator inputs.
///
/// Setting `E = Generator` on `Input<Generator>` adds interactive
/// presentation extras: prompt messages, widget hints, and per-option
/// display labels / separators.
///
/// `Object` is excluded from `SUPPORTED` until the interactive collect loop
/// gains group-prompting support.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq)]
pub struct Generator;

/// Presentation extras shared by every input in a generator schema.
///
/// Flattened into each `Input<Generator>` variant via `E::Base`.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Hash,
)]
pub struct GenBase {
    /// Prompt label shown to the user during interactive collection.
    pub message: String,
    /// When `true`, the collected value is persisted across invocations.
    /// Setting both `secret` and `remember` on the same input is a hard
    /// validation error — they have contradictory semantics.
    #[serde(default)]
    pub remember: bool,
    /// Tera expression evaluated against the collect context to produce this
    /// input's default value when no static `default` is set.
    /// Applies to scalar inputs (boolean, string, integer, float).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_expr: Option<String>,
}

impl GenBase {
    /// Construct a `GenBase` with the given prompt message and all other fields at their defaults.
    pub fn new(message: impl Into<String>) -> Self {
        GenBase {
            message: message.into(),
            ..Default::default()
        }
    }
}

/// Widget hint for string scalar inputs.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
)]
#[serde(rename_all = "kebab-case")]
pub enum StringWidget {
    #[default]
    Text,
    Password,
    Select,
}

/// Flattened extras for string scalar inputs in a generator schema.
///
/// Provides an optional `widget` override; defaults to `Text` when absent.
/// This is `E::String` for `Generator`.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Default, PartialEq,
)]
pub struct StringExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<StringWidget>,
}

/// Widget hint for array inputs.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
)]
#[serde(rename_all = "kebab-case")]
pub enum ListWidget {
    #[default]
    MultiSelect,
    FreeEntry,
}

/// Flattened extras for array inputs in a generator schema.
///
/// Provides an optional `widget` override; defaults to `MultiSelect` when absent.
/// This is `E::Array` for `Generator`.
#[derive(
    Serialize, Deserialize, JsonSchema, Debug, Clone, Default, PartialEq,
)]
pub struct ArrayExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<ListWidget>,
}

/// Presentation extras attached to each `AllowedValue` entry.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Hash,
)]
pub struct AllowedValueExtras {
    /// Display label shown in interactive pickers; defaults to `value` when absent.
    pub name: Option<String>,

    /// When `true` a visual separator is rendered above this option in lists.
    #[serde(default)]
    pub separator: bool,
}

impl InputProfile for Generator {
    /// Object inputs are not yet supported in the interactive collect loop.
    const SUPPORTED: EnumSet<InputKind> = enum_set!(
        InputKind::Boolean
            | InputKind::String
            | InputKind::Integer
            | InputKind::Float
            | InputKind::StringArray
            | InputKind::IntegerArray
            | InputKind::FloatArray
    );

    type Base = GenBase;
    type Boolean = ();
    type String = StringExtras;
    type Numeric = ();
    type Array = ArrayExtras;
    type Object = ();
    type AllowedValueBase = AllowedValueExtras;

    fn is_remember(base: &Self::Base) -> bool {
        base.remember
    }

    fn dynamic_default_expr(base_extra: &Self::Base) -> Option<&str> {
        base_extra.default_expr.as_deref()
    }

    /// Widget override takes priority. For `Text` (the default), data signals
    /// drive inference in `collect()` — no hint returned.
    fn string_presentation_hint(
        input: &StringInput<Generator>,
    ) -> Vec<Cow<'static, str>> {
        if let Some(widget) = input.profile_data.widget {
            match widget {
                StringWidget::Text => vec![],
                StringWidget::Password => vec![Cow::Borrowed("password")],
                StringWidget::Select => {
                    vec![Cow::Borrowed("select"), Cow::Borrowed("text")]
                }
            }
        } else {
            vec![]
        }
    }

    fn string_array_presentation_hint(
        input: &StringArrayInput<Generator>,
    ) -> Vec<Cow<'static, str>> {
        if let Some(widget) = input.profile_data.widget {
            match widget {
                ListWidget::MultiSelect => vec![],
                ListWidget::FreeEntry => vec![Cow::Borrowed("free-entry")],
            }
        } else {
            vec![]
        }
    }

    fn integer_array_presentation_hint(
        input: &IntegerArrayInput<Generator>,
    ) -> Vec<Cow<'static, str>> {
        if let Some(widget) = input.profile_data.widget {
            match widget {
                ListWidget::MultiSelect => vec![],
                ListWidget::FreeEntry => vec![Cow::Borrowed("free-entry")],
            }
        } else {
            vec![]
        }
    }

    fn float_array_presentation_hint(
        input: &FloatArrayInput<Generator>,
    ) -> Vec<Cow<'static, str>> {
        if let Some(widget) = input.profile_data.widget {
            match widget {
                ListWidget::MultiSelect => vec![],
                ListWidget::FreeEntry => vec![Cow::Borrowed("free-entry")],
            }
        } else {
            vec![]
        }
    }

    fn allowed_value_display_name<T>(
        option: &omni_input_schema::AllowedValue<T, Self>,
    ) -> Option<Cow<'_, str>> {
        option
            .base_extra
            .name
            .as_ref()
            .map(|s| Cow::Borrowed(s.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use maps::UnorderedMap;
    use omni_input_schema::{Input, InputKind, ValidationConfig, validate};
    use schemars::schema_for;
    use value_bag::ValueBag;

    use super::*;

    fn assert_round_trip(json: &str) {
        let v1: Input<Generator> =
            serde_json::from_str(json).expect("first deserialization");
        let json2 = serde_json::to_string(&v1).expect("serialization");
        let v2: Input<Generator> =
            serde_json::from_str(&json2).expect("second deserialization");
        assert_eq!(
            v1, v2,
            "round-trip failed.\noriginal={json}\nre-serialized={json2}"
        );
    }

    fn empty_ctx() -> omni_tera::Context {
        omni_tera::Context::new()
    }

    // ── serde round-trips ─────────────────────────────────────────────────────

    #[test]
    fn boolean_round_trip() {
        assert_round_trip(
            r#"{"type":"boolean","name":"flag","message":"Enable?","default":true}"#,
        );
    }

    #[test]
    fn string_round_trip() {
        assert_round_trip(
            r#"{"type":"string","name":"license","message":"Choose license","allowed":[{"value":"mit","name":"MIT"}]}"#,
        );
    }

    #[test]
    fn string_with_widget_override_round_trip() {
        assert_round_trip(
            r#"{"type":"string","name":"token","message":"Enter token","widget":"password"}"#,
        );
    }

    #[test]
    fn integer_round_trip() {
        assert_round_trip(
            r#"{"type":"integer","name":"count","message":"How many?","default":3}"#,
        );
    }

    #[test]
    fn float_round_trip() {
        assert_round_trip(
            r#"{"type":"float","name":"rate","message":"What rate?","default":1.5}"#,
        );
    }

    #[test]
    fn string_array_round_trip() {
        assert_round_trip(
            r#"{"type":"string-array","name":"tags","message":"Add tags"}"#,
        );
    }

    #[test]
    fn integer_array_round_trip() {
        assert_round_trip(
            r#"{"type":"integer-array","name":"ids","message":"Choose IDs","default":[1,2,3]}"#,
        );
    }

    #[test]
    fn float_array_round_trip() {
        assert_round_trip(
            r#"{"type":"float-array","name":"scores","message":"Add scores"}"#,
        );
    }

    #[test]
    fn array_with_free_entry_widget_round_trip() {
        assert_round_trip(
            r#"{"type":"string-array","name":"tags","message":"Add tags","widget":"free-entry"}"#,
        );
    }

    // ── to_data() drops all extras ────────────────────────────────────────────

    #[test]
    fn to_data_drops_gen_base() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"boolean","name":"flag","message":"Enable?","remember":true}"#,
        )
        .unwrap();
        let json = serde_json::to_string(&input.to_data()).unwrap();
        assert!(!json.contains("message"), "message leaked: {json}");
        assert!(!json.contains("remember"), "remember leaked: {json}");
        assert!(json.contains(r#""name":"flag""#));
    }

    #[test]
    fn to_data_drops_string_extras_and_option_extras() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"string","name":"c","message":"m","widget":"select","allowed":[{"value":"a","name":"Option A"}]}"#,
        )
        .unwrap();
        let json = serde_json::to_string(&input.to_data()).unwrap();
        assert!(!json.contains("message"), "message leaked");
        assert!(!json.contains("widget"), "widget leaked");
        assert!(!json.contains(r#""name":"Option A""#), "option name leaked");
        assert!(json.contains(r#""value":"a""#));
    }

    #[test]
    fn to_data_kind_preserved_for_all_variants() {
        let cases: &[(&str, InputKind)] = &[
            (
                r#"{"type":"boolean","name":"a","message":"m"}"#,
                InputKind::Boolean,
            ),
            (
                r#"{"type":"string","name":"a","message":"m"}"#,
                InputKind::String,
            ),
            (
                r#"{"type":"integer","name":"a","message":"m"}"#,
                InputKind::Integer,
            ),
            (
                r#"{"type":"float","name":"a","message":"m"}"#,
                InputKind::Float,
            ),
            (
                r#"{"type":"string-array","name":"a","message":"m"}"#,
                InputKind::StringArray,
            ),
            (
                r#"{"type":"integer-array","name":"a","message":"m"}"#,
                InputKind::IntegerArray,
            ),
            (
                r#"{"type":"float-array","name":"a","message":"m"}"#,
                InputKind::FloatArray,
            ),
        ];
        for (json, expected) in cases {
            let data: Input<Generator> = serde_json::from_str(json).unwrap();
            assert_eq!(data.to_data().kind(), *expected);
        }
    }

    // ── JSON Schema gates Object arm ──────────────────────────────────────────

    #[test]
    fn json_schema_excludes_object_arm() {
        let schema = schema_for!(Input<Generator>);
        let json =
            serde_json::to_string_pretty(&schema).expect("schema to json");
        assert!(
            !json.contains(r#""const": "object""#),
            "Object arm present: {json}"
        );
        for kind in &[
            "boolean",
            "string",
            "integer",
            "float",
            "string-array",
            "integer-array",
            "float-array",
        ] {
            assert!(
                json.contains(&format!(r#""const": "{kind}""#)),
                "Missing arm for '{kind}'"
            );
        }
    }

    // ── presentation hints ────────────────────────────────────────────────────

    #[test]
    fn text_widget_returns_empty_hints() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"string","name":"a","message":"m"}"#,
        )
        .unwrap();
        let Input::String(s) = &input else { panic!() };
        assert!(Generator::string_presentation_hint(s).is_empty());
    }

    #[test]
    fn password_widget_returns_password_hint() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"string","name":"tok","message":"m","widget":"password"}"#,
        )
        .unwrap();
        let Input::String(s) = &input else { panic!() };
        assert_eq!(Generator::string_presentation_hint(s), vec!["password"]);
    }

    #[test]
    fn select_widget_returns_select_then_text_fallback() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"string","name":"e","message":"m","widget":"select"}"#,
        )
        .unwrap();
        let Input::String(s) = &input else { panic!() };
        assert_eq!(
            Generator::string_presentation_hint(s),
            vec!["select", "text"]
        );
    }

    #[test]
    fn multi_select_widget_returns_empty_hints() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"string-array","name":"t","message":"m"}"#,
        )
        .unwrap();
        let Input::StringArray(sa) = &input else {
            panic!()
        };
        assert!(Generator::string_array_presentation_hint(sa).is_empty());
    }

    #[test]
    fn free_entry_widget_returns_free_entry_hint() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"string-array","name":"t","message":"m","widget":"free-entry"}"#,
        )
        .unwrap();
        let Input::StringArray(sa) = &input else {
            panic!()
        };
        assert_eq!(
            Generator::string_array_presentation_hint(sa),
            vec!["free-entry"]
        );
    }

    // ── dynamic_default_expr ──────────────────────────────────────────────────

    #[test]
    fn gen_base_default_expr_round_trips() {
        let json = r#"{"type":"boolean","name":"flag","message":"Enable?","default_expr":"{{ env == 'prod' }}"}"#;
        let input: Input<Generator> = serde_json::from_str(json).unwrap();
        let Input::Boolean(b) = &input else { panic!() };
        assert_eq!(
            b.base_extra.default_expr.as_deref(),
            Some("{{ env == 'prod' }}")
        );
        // to_data() must strip default_expr (it lives in base_extra)
        let data_json = serde_json::to_string(&input.to_data()).unwrap();
        assert!(
            !data_json.contains("default_expr"),
            "default_expr leaked: {data_json}"
        );
    }

    #[test]
    fn dynamic_default_expr_returns_none_without_field() {
        let json = r#"{"type":"boolean","name":"flag","message":"Enable?"}"#;
        let input: Input<Generator> = serde_json::from_str(json).unwrap();
        assert_eq!(Generator::dynamic_default_expr(input.base_extra()), None);
    }

    #[test]
    fn dynamic_default_expr_returns_some_when_set() {
        let json = r#"{"type":"integer","name":"port","message":"Port?","default_expr":"{{ base_port }}"}"#;
        let input: Input<Generator> = serde_json::from_str(json).unwrap();
        assert_eq!(
            Generator::dynamic_default_expr(input.base_extra()),
            Some("{{ base_port }}")
        );
    }

    // ── validate: secret + remember conflict ──────────────────────────────────

    #[test]
    fn secret_plus_remember_is_hard_error() {
        let input: Input<Generator> = serde_json::from_str(
            r#"{"type":"string","name":"tok","message":"m","secret":true,"remember":true}"#,
        )
        .unwrap();
        let mut values = UnorderedMap::default();
        values.insert("tok".to_string(), ValueBag::from_str("x").to_owned());
        let result = validate(
            &[input],
            &values,
            &empty_ctx(),
            &ValidationConfig::default(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tok"));
    }
}
