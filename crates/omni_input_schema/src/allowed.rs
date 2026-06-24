use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::InputProfile;

/// A single allowed value with an optional per-option description and extra
/// presentation fields selected by `TOpt`.
///
/// Serializes as a struct; deserializes from either the full struct form or a
/// bare value shorthand (RFC 0003 §5.4):
///
/// ```yaml
/// # bare shorthand:
/// allowed: [mit, apache-2.0]
///
/// # full struct (generator):
/// allowed:
///   - value: mit
///     name: MIT
///   - value: apache-2.0
///     description: "Permissive, patent grant"
/// ```
#[derive(Serialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(
    serialize = "T: Serialize",
    deserialize = "T: Deserialize<'de>"
))]
#[schemars(
    bound(serialize = "T: JsonSchema", deserialize = "T: JsonSchema"),
    with = "Helper<T, E>"
)]
pub struct AllowedValue<T = String, E: InputProfile = ()> {
    /// The constrained value — typed: `String`, `i64`, or `f64`.
    pub value: T,
    /// Machine-facing per-option help text; emitted in JSON Schema when present.
    pub description: Option<String>,
    /// Presentation extras (e.g. display label, separator).
    #[serde(flatten)]
    pub base_extra: E::AllowedValueBase,
}
/// Helper for the bare-value / full-struct untagged deserialization.
#[derive(Deserialize, JsonSchema)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
#[schemars(bound(deserialize = "T: JsonSchema"))]
struct AllowedValueFull<T, E: InputProfile> {
    pub value: T,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub base_extra: E::AllowedValueBase,
}

// Use an untagged enum: try the full struct first; if that fails
// (the input is a bare scalar), fall back to just T.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged, bound(deserialize = "T: Deserialize<'de>"))]
#[schemars(bound(deserialize = "T: JsonSchema"))]
enum Helper<T, E: InputProfile> {
    Full(AllowedValueFull<T, E>),
    Bare(T),
}

impl<'de, T, E> Deserialize<'de> for AllowedValue<T, E>
where
    T: Deserialize<'de>,
    E: InputProfile,
{
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        match Helper::<T, E>::deserialize(deserializer)? {
            Helper::Full(full) => Ok(AllowedValue {
                value: full.value,
                description: full.description,
                base_extra: full.base_extra,
            }),
            Helper::Bare(value) => Ok(AllowedValue {
                value,
                description: None,
                base_extra: Default::default(),
            }),
        }
    }
}

/// Body shared by all array variants: optional allowed-value list and default.
///
/// `T` is the element type (`String`, `i64`, `f64`).
/// `TOpt` is the per-option extra type from the active `InputProfile`.
///
/// No `Eq` or `Hash` derives — `T` may be `f64`.
#[derive(Serialize, Deserialize, JsonSchema, Debug, Clone, PartialEq)]
#[serde(bound(
    serialize = "T: Serialize",
    deserialize = "T: Deserialize<'de>"
))]
#[schemars(bound(serialize = "T: JsonSchema", deserialize = "T: JsonSchema"))]
pub struct ArrayBody<T = String, E: InputProfile = ()> {
    /// When `Some` the input is constrained to this list; `None` → free entry.
    pub allowed: Option<Vec<AllowedValue<T, E>>>,
}

impl<E: InputProfile> From<&str> for AllowedValue<String, E> {
    fn from(value: &str) -> Self {
        AllowedValue {
            value: value.to_string(),
            description: None,
            base_extra: Default::default(),
        }
    }
}

impl<E: InputProfile> From<String> for AllowedValue<String, E> {
    fn from(value: String) -> Self {
        AllowedValue {
            value,
            description: None,
            base_extra: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_string_shorthand_deserializes() {
        let v: AllowedValue<String> = serde_json::from_str(r#""mit""#).unwrap();
        assert_eq!(v.value, "mit");
        assert_eq!(v.description, None);
        assert_eq!(v.base_extra, ());
    }

    #[test]
    fn bare_integer_shorthand_deserializes() {
        let v: AllowedValue<i64> = serde_json::from_str("3000").unwrap();
        assert_eq!(v.value, 3000);
        assert_eq!(v.description, None);
        assert_eq!(v.base_extra, ());
    }

    #[test]
    fn bare_float_shorthand_deserializes() {
        let v: AllowedValue<f64> = serde_json::from_str("3.14").unwrap();
        assert!((v.value - 3.14).abs() < 1e-10);
        assert_eq!(v.description, None);
        assert_eq!(v.base_extra, ());
    }

    #[test]
    fn full_struct_round_trip() {
        let json = r#"{"value":"mit","description":"MIT License"}"#;
        let v: AllowedValue<String> = serde_json::from_str(json).unwrap();
        assert_eq!(v.value, "mit");
        assert_eq!(v.description.as_deref(), Some("MIT License"));
        let serialized = serde_json::to_string(&v).unwrap();
        let back: AllowedValue<String> =
            serde_json::from_str(&serialized).unwrap();
        assert_eq!(v, back);
    }
}
