use std::borrow::Borrow;

use serde_validate::{StaticValidator, declare_static_validator};

#[derive(Debug, Clone, Copy, Default)]
pub struct RegexValidator;

impl<V: Borrow<String>> StaticValidator<V> for RegexValidator {
    fn validate_static(value: &V) -> Result<(), String> {
        if let Err(error) = regex::Regex::new(value.borrow()) {
            return Err(error.to_string());
        }

        Ok(())
    }
}

declare_static_validator!(
    RegexValidator,
    String,
    validate_regex,
    option_validate_regex
);
