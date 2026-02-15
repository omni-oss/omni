use derive_new::new;
use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::{DictConfig, ListConfig};

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate, new,
)]
#[serde(untagged)]
#[garde(allow_unvalidated)]
pub enum DynValue {
    Boolean(#[new(into)] bool),
    Integer(#[new(into)] i64),
    Float(#[new(into)] f64),
    String(#[new(into)] String),
    List(#[new(into)] ListConfig<DynValue>),
    Dict(#[new(into)] DictConfig<DynValue>),
}

impl Merge for DynValue {
    fn merge(&mut self, other: Self) {
        match (self, other) {
            (DynValue::List(a), DynValue::List(b)) => {
                a.merge(b);
            }
            (DynValue::Dict(a), DynValue::Dict(b)) => {
                a.merge(b);
            }
            (this, other) => {
                *this = other;
            }
        }
    }
}

impl DynValue {
    pub fn into_json(self) -> JsonValue {
        match self {
            DynValue::Boolean(b) => JsonValue::Bool(b),
            DynValue::Integer(i) => JsonValue::Number(
                serde_json::Number::from_i128(i as i128)
                    .expect("should be valid"),
            ),
            DynValue::Float(f) => JsonValue::Number(
                serde_json::Number::from_f64(f).expect("should be valid"),
            ),
            DynValue::String(s) => JsonValue::String(s),
            DynValue::List(list_config) => JsonValue::Array(
                list_config
                    .to_vec()
                    .into_iter()
                    .map(DynValue::into_json)
                    .collect(),
            ),
            DynValue::Dict(dict_config) => JsonValue::Object(
                dict_config
                    .into_map()
                    .into_iter()
                    .map(|(k, v)| (k, DynValue::into_json(v)))
                    .collect::<serde_json::Map<_, _>>(),
            ),
        }
    }
}
