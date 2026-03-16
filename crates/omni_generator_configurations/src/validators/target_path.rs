#![allow(unused)]

use maps::UnorderedMap;
use omni_serde_validators::tera_expr::{TeraExprValidator, validate_str};
use serde_validate::{StaticValidator, declare_static_validator};
use std::{borrow::Borrow, path::PathBuf};

use crate::OmniPath;

#[derive(Debug, Clone, Copy, Default)]
pub struct TargetPathValidator;

impl<V: Borrow<OmniPath>> StaticValidator<V> for TargetPathValidator {
    fn validate_static(value: &V) -> Result<(), String> {
        let value_str = value.borrow().unresolved_path().to_string_lossy();
        validate_str(&value_str)?;

        if value.borrow().unresolved_path().is_absolute() {
            return Err("path should not be absolute".to_string());
        }

        Ok(())
    }
}

declare_static_validator!(
    TargetPathValidator,
    OmniPath,
    validate_target_path,
    option_validate_target_path
);

#[derive(Debug, Clone, Copy, Default)]
pub struct UMapTargetPathValidator;

impl<V: Borrow<UnorderedMap<String, OmniPath>>> StaticValidator<V>
    for UMapTargetPathValidator
{
    fn validate_static(value: &V) -> Result<(), String> {
        for value in value.borrow().values() {
            TargetPathValidator::validate_static(value)?;
        }
        Ok(())
    }
}

declare_static_validator!(
    UMapTargetPathValidator,
    UnorderedMap<String, OmniPath>,
    validate_umap_target_path,
    option_umap_validate_target_path
);
