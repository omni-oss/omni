use config_utils::{DictConfig, ListConfig};
use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Deserialize,
    Serialize,
    JsonSchema,
    Clone,
    Debug,
    PartialEq,
    Merge,
    Validate,
    Default,
)]
#[garde(allow_unvalidated)]
pub struct MetaConfiguration(DictConfig<MetaValue>);

#[derive(
    Deserialize, Serialize, JsonSchema, Clone, Debug, PartialEq, Validate,
)]
#[serde(untagged)]
#[garde(allow_unvalidated)]
pub enum MetaValue {
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    List(ListConfig<MetaValue>),
    Dict(DictConfig<MetaValue>),
}

impl Merge for MetaValue {
    fn merge(&mut self, other: Self) {
        match (self, other) {
            (MetaValue::List(a), MetaValue::List(b)) => {
                a.merge(b);
            }
            (MetaValue::Dict(a), MetaValue::Dict(b)) => {
                a.merge(b);
            }
            (this, other) => {
                *this = other;
            }
        }
    }
}
