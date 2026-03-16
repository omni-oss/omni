#![allow(unused)]

use maps::UnorderedMap;
use omni_serde_validators::tera_expr::TeraExprValidator;
use serde_validate::{StaticValidator, declare_static_validator};
use std::{borrow::Borrow, path::PathBuf};

#[derive(Debug, Clone, Copy, Default)]
pub struct SerdeJsonValidator;

impl<V: Borrow<serde_json::Value>> StaticValidator<V> for SerdeJsonValidator {
    fn validate_static(value: &V) -> Result<(), String> {
        match value.borrow() {
            serde_json::Value::String(s) => {
                TeraExprValidator::validate_static(s)?
            }
            serde_json::Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    Self::validate_static(value).map_err(|e| {
                        format!("value at index {} is invalid: {}", index, e)
                    })?;
                }
            }
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    Self::validate_static(value).map_err(|e| {
                        format!("value for key {} is invalid: {}", key, e)
                    })?;
                }
            }
            _ => {
                // only strings, arrays of strings and objects with string values should be validated
            }
        }

        Ok(())
    }
}

declare_static_validator!(
    SerdeJsonValidator,
    serde_json::Value,
    validate_serde_json,
    option_validate_serde_json
);

#[derive(Debug, Clone, Copy, Default)]
pub struct UmapSerdeJsonValidator;

impl<V: Borrow<UnorderedMap<String, serde_json::Value>>> StaticValidator<V>
    for UmapSerdeJsonValidator
{
    fn validate_static(value: &V) -> Result<(), String> {
        for (key, value) in value.borrow().iter() {
            SerdeJsonValidator::validate_static(value).map_err(|e| {
                format!("value for key {} is invalid: {}", key, e)
            })?;
        }

        Ok(())
    }
}

declare_static_validator!(
    UmapSerdeJsonValidator,
    UnorderedMap<String, serde_json::Value>,
    validate_umap_serde_json,
    option_validate_umap_serde_json
);
