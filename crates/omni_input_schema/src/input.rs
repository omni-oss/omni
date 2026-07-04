use maps::UnorderedMap;
use omni_config_types::{MaybeExpr, TeraExpr};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

use crate::allowed::{AllowedValue, ArrayBody};
use crate::base::BaseInput;
use crate::profile::InputProfile;

// ── Named inner structs ───────────────────────────────────────────────────────

// **IMPORTANT**
// Why deserialize and serialize with `bound(deserialize = "", serialize = "")`?
// Because E: InputProfile is just not actually used as fields.
// Instead it has associated types that are used as fields, and those associated types
// already have constraints on them so adding them here is redundant.

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct BooleanInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    pub default: Option<MaybeExpr<bool>>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub boolean_extra: E::Boolean,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct StringInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    pub allowed: Option<Vec<AllowedValue<String, E>>>,
    pub default: Option<String>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub string_extra: E::String,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct IntegerInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    pub allowed: Option<Vec<AllowedValue<i64, E>>>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub numeric_extra: E::Numeric,
    #[serde(default)]
    pub default: Option<MaybeExpr<i64>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct FloatInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    pub allowed: Option<Vec<AllowedValue<f64, E>>>,
    #[serde(default)]
    pub default: Option<MaybeExpr<f64>>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub numeric_extra: E::Numeric,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct StringArrayInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    #[serde(flatten)]
    pub body: ArrayBody<String, E>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub array_extra: E::Array,
    #[serde(default)]
    pub default: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct IntegerArrayInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    #[serde(flatten)]
    pub body: ArrayBody<i64, E>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub array_extra: E::Array,
    #[serde(default)]
    pub default: Option<Vec<i64>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct FloatArrayInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    #[serde(flatten)]
    pub body: ArrayBody<f64, E>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub array_extra: E::Array,
    #[serde(default)]
    pub default: Option<Vec<f64>>,
}

/// Nested object: a group of typed fields returned as one JSON object value.
/// Available immediately for tools / plugins / MCP (`Input<()>`).
/// Gated for generators via `SUPPORTED` until group-prompting is implemented.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(deserialize = "", serialize = ""), deny_unknown_fields)]
#[schemars(bound(deserialize = "", serialize = ""))]
pub struct ObjectInput<E: InputProfile = ()> {
    #[serde(flatten)]
    pub base: BaseInput,
    pub fields: Vec<Input<E>>,
    #[serde(default)]
    pub default: Option<UnorderedMap<String, InputValue>>,
    #[serde(flatten)]
    pub base_extra: E::Base,
    #[serde(flatten)]
    pub object_extra: E::Object,
}

#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum InputValue {
    Boolean(bool),
    String(String),
    Integer(i64),
    Float(f64),
    StringArray(Vec<String>),
    IntegerArray(Vec<i64>),
    FloatArray(Vec<f64>),
    Object(UnorderedMap<String, InputValue>),
}

// ── Input enum ────────────────────────────────────────────────────────────────

/// A single typed input, generic over the extras family `E`.
///
/// `E = ()` — pure data layer; used by tools, plugins, and MCP.
/// `E = Generator` (in `omni_generator_configurations`) — adds message, widget hints, options labels.
///
/// The `type` tag in YAML/JSON is the data type:
/// `"boolean"`, `"string"`, `"integer"`, `"float"`,
/// `"string-array"`, `"integer-array"`, `"float-array"`, `"object"`.
///
/// `JsonSchema` is **not** derived — a manual impl in `json_schema.rs` iterates
/// `E::SUPPORTED` so unsupported variants are excluded by construction.
///
/// `Eq` and `Hash` are **not** derived — `Float` / `FloatArray` carry `f64`.
#[derive(
    Serialize, Deserialize, Debug, Clone, PartialEq, EnumDiscriminants,
)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    bound(deserialize = "", serialize = "")
)]
#[strum_discriminants(
    derive(EnumIs, enumset::EnumSetType, Serialize, Deserialize, JsonSchema),
    name(InputKind),
    enumset(no_super_impls),
    serde(rename_all = "kebab-case")
)]
pub enum Input<E: InputProfile = ()> {
    Boolean(BooleanInput<E>),
    String(StringInput<E>),
    Integer(IntegerInput<E>),
    Float(FloatInput<E>),
    StringArray(StringArrayInput<E>),
    IntegerArray(IntegerArrayInput<E>),
    FloatArray(FloatArrayInput<E>),
    Object(ObjectInput<E>),
}

impl<E: InputProfile> Input<E> {
    /// The data-type discriminant of this input, presentation-free.
    pub fn kind(&self) -> InputKind {
        self.discriminant()
    }

    /// The profile-specific base extras shared by every `Input<E>` variant.
    pub fn base_extra(&self) -> &E::Base {
        match self {
            Input::Boolean(b) => &b.base_extra,
            Input::String(s) => &s.base_extra,
            Input::Integer(i) => &i.base_extra,
            Input::Float(f) => &f.base_extra,
            Input::StringArray(sa) => &sa.base_extra,
            Input::IntegerArray(ia) => &ia.base_extra,
            Input::FloatArray(fa) => &fa.base_extra,
            Input::Object(o) => &o.base_extra,
        }
    }

    /// The shared data fields every consumer reads.
    pub fn base(&self) -> &BaseInput {
        match self {
            Input::Boolean(b) => &b.base,
            Input::String(s) => &s.base,
            Input::Integer(i) => &i.base,
            Input::Float(f) => &f.base,
            Input::StringArray(sa) => &sa.base,
            Input::IntegerArray(ia) => &ia.base,
            Input::FloatArray(fa) => &fa.base,
            Input::Object(o) => &o.base,
        }
    }

    /// Strip all presentation extras, returning a pure-data `Input<()>`.
    ///
    /// The projection is total and mechanical — exhaustive match, no per-widget
    /// logic.  For `Object`, recurses into `fields` (RFC 0003 decision 4).
    pub fn to_data(&self) -> Input<()> {
        match self {
            Input::Boolean(b) => Input::Boolean(BooleanInput {
                base: b.base.clone(),
                default: b.default.clone(),
                base_extra: (),
                boolean_extra: (),
            }),
            Input::String(s) => Input::String(StringInput {
                base: s.base.clone(),
                allowed: s.allowed.as_ref().map(|v| {
                    v.iter()
                        .map(|a| AllowedValue {
                            value: a.value.clone(),
                            description: a.description.clone(),
                            base_extra: (),
                        })
                        .collect()
                }),
                default: s.default.clone(),
                base_extra: (),
                string_extra: (),
            }),
            Input::Integer(i) => Input::Integer(IntegerInput {
                base: i.base.clone(),
                allowed: i.allowed.as_ref().map(|v| {
                    v.iter()
                        .map(|a| AllowedValue {
                            value: a.value,
                            description: a.description.clone(),
                            base_extra: (),
                        })
                        .collect()
                }),
                default: i.default.clone(),
                base_extra: (),
                numeric_extra: (),
            }),
            Input::Float(f) => Input::Float(FloatInput {
                base: f.base.clone(),
                allowed: f.allowed.as_ref().map(|v| {
                    v.iter()
                        .map(|a| AllowedValue {
                            value: a.value,
                            description: a.description.clone(),
                            base_extra: (),
                        })
                        .collect()
                }),
                default: f.default.clone(),
                base_extra: (),
                numeric_extra: (),
            }),
            Input::StringArray(sa) => Input::StringArray(StringArrayInput {
                base: sa.base.clone(),
                body: ArrayBody {
                    allowed: sa.body.allowed.as_ref().map(|v| {
                        v.iter()
                            .map(|a| AllowedValue {
                                value: a.value.clone(),
                                description: a.description.clone(),
                                base_extra: (),
                            })
                            .collect()
                    }),
                },
                base_extra: (),
                default: sa.default.clone(),
                array_extra: (),
            }),
            Input::IntegerArray(ia) => Input::IntegerArray(IntegerArrayInput {
                base: ia.base.clone(),
                body: ArrayBody {
                    allowed: ia.body.allowed.as_ref().map(|v| {
                        v.iter()
                            .map(|a| AllowedValue {
                                value: a.value,
                                description: a.description.clone(),
                                base_extra: (),
                            })
                            .collect()
                    }),
                },
                default: ia.default.clone(),
                base_extra: (),
                array_extra: (),
            }),
            Input::FloatArray(fa) => Input::FloatArray(FloatArrayInput {
                base: fa.base.clone(),
                body: ArrayBody {
                    allowed: fa.body.allowed.as_ref().map(|v| {
                        v.iter()
                            .map(|a| AllowedValue {
                                value: a.value,
                                description: a.description.clone(),
                                base_extra: (),
                            })
                            .collect()
                    }),
                },
                default: fa.default.clone(),
                base_extra: (),
                array_extra: (),
            }),
            Input::Object(o) => Input::Object(ObjectInput {
                base: o.base.clone(),
                fields: o.fields.iter().map(|f| f.to_data()).collect(),
                base_extra: (),
                object_extra: (),
                default: o.default.clone(),
            }),
        }
    }

    /// The static default value, if any, for use by `validate` and `inspect`.
    pub fn static_default_value_bag(&self) -> Option<value_bag::OwnedValueBag> {
        use value_bag::ValueBag;
        match self {
            Input::Boolean(b) => b
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|v| ValueBag::capture_serde1(v).to_owned()),
            Input::String(s) => {
                if s.default.as_deref().is_some_and(is_string_expr) {
                    return None;
                }

                s.default
                    .as_ref()
                    .map(|v| ValueBag::from_serde1(v).to_owned())
            }
            Input::Integer(i) => i
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|v| ValueBag::capture_serde1(v).to_owned()),
            Input::Float(f) => f
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_value_ref)
                .map(|v| ValueBag::capture_serde1(v).to_owned()),
            Input::StringArray(sa) => sa
                .default
                .as_ref()
                .map(|v| ValueBag::from_serde1(v).to_owned()),
            Input::IntegerArray(ia) => ia
                .default
                .as_ref()
                .map(|v| ValueBag::from_serde1(v).to_owned()),
            Input::FloatArray(fa) => fa
                .default
                .as_ref()
                .map(|v| ValueBag::from_serde1(v).to_owned()),
            Input::Object(o) => o
                .default
                .as_ref()
                .map(|v| ValueBag::from_serde1(v).to_owned()),
        }
    }

    pub fn dynamic_default_expr(&self) -> Option<&str> {
        match self {
            Input::Boolean(b) => b
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_expr_ref)
                .map(TeraExpr::as_str),
            Input::String(s) => {
                let str = s.default.as_deref();
                if str.is_some_and(is_string_expr) {
                    str
                } else {
                    None
                }
            }
            Input::Integer(i) => i
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_expr_ref)
                .map(TeraExpr::as_str),
            Input::Float(f) => f
                .default
                .as_ref()
                .and_then(MaybeExpr::try_as_expr_ref)
                .map(TeraExpr::as_str),
            Input::StringArray(_) => None,
            Input::IntegerArray(_) => None,
            Input::FloatArray(_) => None,
            Input::Object(_) => None,
        }
    }
}

fn is_string_expr(value: &str) -> bool {
    value.contains("{{") || value.contains("{%")
}

/// Type alias for the pure-data input type used by tools, plugins, and MCP.
pub type InputSchema = Input<()>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Deserialize from JSON, serialize back, deserialize again — both
    /// deserialized values must be equal.  This exercises serde round-trip
    /// correctness for `Input<()>` without needing to construct values manually.
    fn assert_round_trip(json: &str) {
        let v1: Input<()> =
            serde_json::from_str(json).expect("first deserialization");
        let json2 = serde_json::to_string(&v1).expect("serialization");
        let v2: Input<()> =
            serde_json::from_str(&json2).expect("second deserialization");
        assert_eq!(
            v1, v2,
            "round trip failed.\noriginal={json}\nre-serialized={json2}"
        );
    }

    #[test]
    fn boolean_round_trip() {
        assert_round_trip(r#"{"type":"boolean","name":"flag","default":true}"#);
    }

    #[test]
    fn string_round_trip() {
        assert_round_trip(
            r#"{"type":"string","name":"choice","allowed":["a","b"],"default":"a"}"#,
        );
    }

    #[test]
    fn integer_round_trip() {
        assert_round_trip(r#"{"type":"integer","name":"count","default":3}"#);
    }

    #[test]
    fn float_round_trip() {
        assert_round_trip(r#"{"type":"float","name":"rate","default":1.5}"#);
    }

    #[test]
    fn string_array_round_trip() {
        assert_round_trip(r#"{"type":"string-array","name":"tags"}"#);
    }

    #[test]
    fn integer_array_round_trip() {
        assert_round_trip(
            r#"{"type":"integer-array","name":"ids","default":[1,2,3]}"#,
        );
    }

    #[test]
    fn float_array_round_trip() {
        assert_round_trip(r#"{"type":"float-array","name":"scores"}"#);
    }

    #[test]
    fn object_round_trip() {
        assert_round_trip(
            r#"{"type":"object","name":"group","fields":[{"type":"boolean","name":"enabled","default":false}]}"#,
        );
    }

    #[test]
    fn kind_returns_correct_discriminant_for_all_variants() {
        let cases: &[(&str, InputKind)] = &[
            (r#"{"type":"boolean","name":"a"}"#, InputKind::Boolean),
            (r#"{"type":"string","name":"a"}"#, InputKind::String),
            (r#"{"type":"integer","name":"a"}"#, InputKind::Integer),
            (r#"{"type":"float","name":"a"}"#, InputKind::Float),
            (
                r#"{"type":"string-array","name":"a"}"#,
                InputKind::StringArray,
            ),
            (
                r#"{"type":"integer-array","name":"a"}"#,
                InputKind::IntegerArray,
            ),
            (
                r#"{"type":"float-array","name":"a"}"#,
                InputKind::FloatArray,
            ),
            (
                r#"{"type":"object","name":"a","fields":[]}"#,
                InputKind::Object,
            ),
        ];
        for (json, expected) in cases {
            let input: Input<()> = serde_json::from_str(json).unwrap();
            assert_eq!(input.kind(), *expected, "kind() mismatch for {json}");
        }
    }

    #[test]
    fn to_data_is_identity_on_unit_profile() {
        let cases: &[&str] = &[
            r#"{"type":"boolean","name":"flag","default":true}"#,
            r#"{"type":"string","name":"choice","allowed":["a","b"]}"#,
            r#"{"type":"integer","name":"count"}"#,
            r#"{"type":"float","name":"rate","default":1.5}"#,
            r#"{"type":"string-array","name":"tags"}"#,
            r#"{"type":"integer-array","name":"ids"}"#,
            r#"{"type":"float-array","name":"scores"}"#,
            r#"{"type":"object","name":"group","fields":[{"type":"boolean","name":"enabled"}]}"#,
        ];
        for json in cases {
            let input: Input<()> = serde_json::from_str(json).unwrap();
            assert_eq!(
                input.to_data(),
                input,
                "to_data() not identity for {json}"
            );
        }
    }

    // ── flatten / deny_unknown_fields behavior ────────────────────────────────

    /// `name`, `if`, `validators`, `secret`, and `description` all live in the
    /// flattened `BaseInput`, reached through the internally-tagged `Input`
    /// enum -> variant struct -> `#[serde(flatten)] base`. This asserts those
    /// deeply-flattened fields are respected on deserialization.
    #[test]
    fn unit_profile_accepts_deeply_nested_flattened_base_fields() {
        let json = r#"{
            "type": "string",
            "name": "token",
            "if": true,
            "secret": true,
            "description": "an API token",
            "validators": [{"condition": true, "error_message": "bad"}],
            "default": "abc"
        }"#;
        let input: Input<()> =
            serde_json::from_str(json).expect("should parse");
        let base = input.base();
        assert_eq!(base.name, "token");
        assert!(base.r#if.is_some());
        assert!(base.secret);
        assert_eq!(base.description.as_deref(), Some("an API token"));
        assert_eq!(base.validators.len(), 1);
        let Input::String(s) = &input else {
            panic!("expected String variant")
        };
        assert_eq!(s.default.as_deref(), Some("abc"));
    }

    /// `deny_unknown_fields` on every `*Input` struct must reject keys that
    /// belong to none of the (possibly deeply-flattened) sub-structs.
    #[test]
    fn unit_profile_rejects_unknown_field() {
        let json = r#"{"type":"string","name":"x","totally_unknown":true}"#;
        let result: Result<Input<()>, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "unknown field should be rejected, got: {result:?}"
        );
    }

    /// `ValidateConfiguration` is `#[serde(deny_unknown_fields)]`; unknown keys
    /// inside a validator entry must be rejected.
    #[test]
    fn validator_rejects_unknown_field() {
        let json = r#"{"type":"string","name":"x","validators":[{"condition":true,"bogus":1}]}"#;
        let result: Result<Input<()>, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "unknown validator field should be rejected, got: {result:?}"
        );
    }

    #[test]
    fn to_data_recurses_object_fields() {
        let json = r#"{
            "type":"object",
            "name":"group",
            "fields":[
                {"type":"boolean","name":"enabled","default":false},
                {"type":"string","name":"label"}
            ]
        }"#;
        let input: Input<()> = serde_json::from_str(json).unwrap();
        let data = input.to_data();

        assert_eq!(data.base().name, "group");

        if let Input::Object(o) = &data {
            assert_eq!(o.fields.len(), 2);
            assert_eq!(o.fields[0].kind(), InputKind::Boolean);
            assert_eq!(o.fields[0].base().name, "enabled");
            assert_eq!(o.fields[1].kind(), InputKind::String);
            assert_eq!(o.fields[1].base().name, "label");
        } else {
            panic!("expected Object variant, got {:?}", data.kind());
        }
    }
}
