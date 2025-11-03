use serde_validate::{StaticValidator, declare_static_validator};

#[derive(Copy, Clone, Default)]
pub struct NameValidator;

impl StaticValidator<String> for NameValidator {
    fn validate_static(value: &String) -> Result<(), String> {
        if value.is_empty() {
            return Err("cannot be empty".to_string());
        }

        let mut chars = value.chars();

        let first_char = chars.next().unwrap();

        if !(first_char.is_ascii_alphabetic() || first_char == '_') {
            return Err("must start with a letter".to_string());
        }

        if chars.any(|c| !c.is_ascii_alphanumeric() && c != '_') {
            return Err("must contain only letters, numbers and underscores"
                .to_string());
        }

        Ok(())
    }
}

declare_static_validator!(NameValidator, String, validate_name);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_name() {
        let validate = NameValidator::validate_static;

        assert!(validate(&"hello".to_string()).is_ok());
        assert!(validate(&"hello_world".to_string()).is_ok());
        assert!(validate(&"HelloWorld".to_string()).is_ok());
        assert!(validate(&"_hello".to_string()).is_ok());
        assert!(validate(&"hello_".to_string()).is_ok());
        assert!(validate(&"hello_123".to_string()).is_ok());

        assert!(validate(&"".to_string()).is_err());
        assert!(validate(&"123".to_string()).is_err());
        assert!(validate(&"hello world".to_string()).is_err());
        assert!(validate(&"hello-world".to_string()).is_err());
        assert!(validate(&"hello world!".to_string()).is_err());
    }
}
