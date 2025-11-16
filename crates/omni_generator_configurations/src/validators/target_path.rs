#![allow(unused)]

use maps::UnorderedMap;
use omni_serde_validators::tera_expr::{TeraExprValidator, validate_str};
use serde_validate::{StaticValidator, declare_static_validator};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default)]
pub struct TargetPathValidator;

impl StaticValidator<PathBuf> for TargetPathValidator {
    fn validate_static(value: &PathBuf) -> Result<(), String> {
        validate_str(value.to_string_lossy().as_ref())?;

        if value.is_absolute() {
            return Err("path should not be absolute".to_string());
        }

        Ok(())
    }
}

declare_static_validator!(
    TargetPathValidator,
    PathBuf,
    validate_target_path,
    option_validate_target_path
);

#[derive(Debug, Clone, Copy, Default)]
pub struct UMapTargetPathValidator;

impl StaticValidator<UnorderedMap<String, PathBuf>>
    for UMapTargetPathValidator
{
    fn validate_static(
        value: &UnorderedMap<String, PathBuf>,
    ) -> Result<(), String> {
        for value in value.values() {
            TargetPathValidator::validate_static(value)?;
        }
        Ok(())
    }
}

declare_static_validator!(
    UMapTargetPathValidator,
    UnorderedMap<String, PathBuf>,
    validate_umap_target_path,
    option_umap_validate_target_path
);
