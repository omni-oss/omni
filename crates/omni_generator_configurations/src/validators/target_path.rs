#![allow(unused)]

use maps::UnorderedMap;
use omni_serde_validators::tera_expr::{TeraExprValidator, validate_str};
use serde_validate::{StaticValidator, declare_static_validator};
use std::path::PathBuf;

use crate::OmniPath;

#[derive(Debug, Clone, Copy, Default)]
pub struct TargetPathValidator;

impl StaticValidator<OmniPath> for TargetPathValidator {
    fn validate_static(value: &OmniPath) -> Result<(), String> {
        validate_str(value.unresolved_path().to_string_lossy().as_ref())?;

        if value.unresolved_path().is_absolute() {
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

impl StaticValidator<UnorderedMap<String, OmniPath>>
    for UMapTargetPathValidator
{
    fn validate_static(
        value: &UnorderedMap<String, OmniPath>,
    ) -> Result<(), String> {
        for value in value.values() {
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
