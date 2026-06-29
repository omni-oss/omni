use std::borrow::Cow;

use maps::UnorderedMap;
use value_bag::OwnedValueBag;

use crate::error::Error;

pub fn get_tera_context(
    context_values: &UnorderedMap<String, OwnedValueBag>,
) -> omni_tera::Context {
    let mut context = omni_tera::context::STANDARD.clone();

    for (key, value) in context_values.iter() {
        context.insert(key.clone(), value);
    }

    context
}

pub fn expand_json_value<'v>(
    tera_ctx: &omni_tera::Context,
    parent_key: Option<&str>,
    key: &str,
    value: &'v serde_json::Value,
) -> Result<Cow<'v, serde_json::Value>, Error> {
    Ok(match value {
        serde_json::Value::String(s) => {
            let expanded = omni_tera::one_off(
                &s,
                &(if let Some(parent_key) = parent_key {
                    format!("value for {}.{}", parent_key, key)
                } else {
                    format!("value for {}", key)
                }),
                tera_ctx,
            )?;

            Cow::Owned(serde_json::Value::String(expanded))
        }
        serde_json::Value::Array(values) => {
            let mut result = Vec::<serde_json::Value>::new();
            for (idx, value) in values.iter().enumerate() {
                let idx_key = idx.to_string();

                let value = (*if let Some(parent) = parent_key {
                    expand_json_value(
                        tera_ctx,
                        Some(&format!("{}.{}", parent, key)),
                        &idx_key,
                        value,
                    )?
                } else {
                    expand_json_value(tera_ctx, Some(key), &idx_key, value)?
                })
                .to_owned();
                result.push(value);
            }

            Cow::Owned(serde_json::Value::Array(result))
        }
        serde_json::Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (map_key, value) in map {
                let value = (*if let Some(parent) = parent_key {
                    expand_json_value(
                        tera_ctx,
                        Some(&format!("{}.{}", parent, key)),
                        &map_key,
                        value,
                    )?
                } else {
                    expand_json_value(tera_ctx, Some(key), &map_key, value)?
                })
                .to_owned();
                result.insert(map_key.to_string(), value);
            }

            Cow::Owned(serde_json::Value::Object(result))
        }
        value @ _ => Cow::Borrowed(value),
    })
}
