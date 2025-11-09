use maps::UnorderedMap;
use serde_validate::{StaticValidator, declare_static_validator};

#[derive(Debug, Clone, Copy, Default)]
pub struct TeraExprValidator;

impl StaticValidator<String> for TeraExprValidator {
    fn validate_static(value: &String) -> Result<(), String> {
        let result = tera::Template::new("__validate_template__", None, value);

        if let Err(error) = result {
            return Err(error.to_string());
        }

        Ok(())
    }
}

declare_static_validator!(
    TeraExprValidator,
    String,
    validate_tera_expr,
    option_validate_tera_expr
);

#[derive(Debug, Clone, Copy, Default)]
pub struct UMapTeraExprValidator;

impl StaticValidator<UnorderedMap<String, String>> for UMapTeraExprValidator {
    fn validate_static(
        value: &UnorderedMap<String, String>,
    ) -> Result<(), String> {
        for value in value.values() {
            TeraExprValidator::validate_static(value)?;
        }

        Ok(())
    }
}

declare_static_validator!(
    UMapTeraExprValidator,
    UnorderedMap<String, String>,
    validate_umap_tera_expr,
    option_validate_umap_tera_expr
);

#[cfg(test)]
mod test {
    use serde_validate::StaticValidator;

    use super::*;

    #[test]
    fn test_tera_expr_validator() {
        let validate = TeraExprValidator::validate_static;

        assert!(validate(&"true".to_string()).is_ok());
        assert!(validate(&"{{ value }}".to_string()).is_ok());
        assert!(validate(&"{ value }".to_string()).is_ok());

        assert!(validate(&"{{ value".to_string()).is_err());
        assert!(validate(&"{{ value }".to_string()).is_err());
    }
}
