#![allow(unused)]

use maps::UnorderedMap;
use serde_validate::{StaticValidator, declare_static_validator};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default)]
pub struct PathNotAbsoluteValidator;

impl StaticValidator<PathBuf> for PathNotAbsoluteValidator {
    fn validate_static(value: &PathBuf) -> Result<(), String> {
        if value.is_absolute() {
            return Err("path should not be absolute".to_string());
        }

        Ok(())
    }
}

declare_static_validator!(
    PathNotAbsoluteValidator,
    PathBuf,
    validate_path_not_absolute,
    option_validate_path_not_absolute
);

#[derive(Debug, Clone, Copy, Default)]
pub struct UMapPathNotAbsoluteValidator;

impl StaticValidator<UnorderedMap<String, PathBuf>>
    for UMapPathNotAbsoluteValidator
{
    fn validate_static(
        value: &UnorderedMap<String, PathBuf>,
    ) -> Result<(), String> {
        for value in value.values() {
            if value.is_absolute() {
                return Err("path should not be absolute".to_string());
            }
        }
        Ok(())
    }
}

declare_static_validator!(
    UMapPathNotAbsoluteValidator,
    UnorderedMap<String, PathBuf>,
    validate_umap_path_not_absolute,
    option_umap_validate_path_not_absolute
);
