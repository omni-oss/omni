use config_utils::{DictConfig, ListConfig};
use garde::Validate;
use merge::Merge;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

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
pub struct MetaConfiguration(pub DictConfig<MetaValue>);

impl MetaConfiguration {
    pub fn into_expression_context(
        self,
    ) -> Result<omni_expressions::Context<'static>, omni_expressions::Error>
    {
        let mut ctx = omni_expressions::Context::default();

        for (k, v) in self.0.iter() {
            ctx.add_variable(k, v.clone().into_json())?;
        }

        Ok(ctx)
    }
}

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

impl MetaValue {
    pub fn into_json(self) -> JsonValue {
        match self {
            MetaValue::Boolean(b) => JsonValue::Bool(b),
            MetaValue::Integer(i) => JsonValue::Number(
                serde_json::Number::from_i128(i as i128)
                    .expect("should be valid"),
            ),
            MetaValue::Float(f) => JsonValue::Number(
                serde_json::Number::from_f64(f).expect("should be valid"),
            ),
            MetaValue::String(s) => JsonValue::String(s),
            MetaValue::List(list_config) => JsonValue::Array(
                list_config
                    .to_vec()
                    .into_iter()
                    .map(MetaValue::into_json)
                    .collect(),
            ),
            MetaValue::Dict(dict_config) => JsonValue::Object(
                dict_config
                    .into_map()
                    .into_iter()
                    .map(|(k, v)| (k, MetaValue::into_json(v)))
                    .collect::<serde_json::Map<_, _>>(),
            ),
        }
    }
}
