use serde_validate::{StaticValidator, declare_static_validator};

#[derive(Debug, Clone, Copy, Default)]
pub struct RegexValidator;

impl StaticValidator<String> for RegexValidator {
    fn validate_static(value: &String) -> Result<(), String> {
        if let Err(error) = regex::Regex::new(&value) {
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
