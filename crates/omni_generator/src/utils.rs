use std::borrow::Cow;

use maps::UnorderedMap;
use value_bag::OwnedValueBag;

use crate::error::Error;

pub fn get_tera_context(
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> tera::Context {
    let mut context = tera::Context::new();

    for (key, value) in context_values.iter() {
        context.insert(key, value);
    }

    context
}

pub fn expand_json_value<'v>(
    tera_ctx: &tera::Context,
    key: &String,
    value: &'v tera::Value,
) -> Result<Cow<'v, tera::Value>, Error> {
    Ok(match value {
        tera::Value::String(s) => {
            let expanded = omni_tera::one_off(
                &s,
                &format!("value for var {}", key),
                tera_ctx,
            )?;

            Cow::Owned(tera::Value::String(expanded))
        }
        tera::Value::Array(values) => {
            let mut result = Vec::<serde_json::Value>::new();
            for value in values {
                let value = (*expand_json_value(tera_ctx, key, value)?).clone();
                result.push(value);
            }

            Cow::Owned(tera::Value::Array(result))
        }
        tera::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                let value = (*expand_json_value(tera_ctx, key, value)?).clone();
                result.insert(key.to_string(), value);
            }

            Cow::Owned(tera::Value::Object(result))
        }
        value @ _ => Cow::Borrowed(value),
    })
}
