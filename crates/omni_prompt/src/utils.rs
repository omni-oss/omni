use either::Either::{Left, Right};
use value_bag::OwnedValueBag;

use crate::{
    configuration::ValidateConfiguration,
    error::{Error, ErrorInner},
};

pub fn validate_value(
    prompt_name: &str,
    value: &OwnedValueBag,
    context_values: &tera::Context,
    validators: &[ValidateConfiguration],
    value_name: Option<&str>,
) -> Result<(), Error> {
    for validator in validators {
        let mut ctx = context_values.clone();
        ctx.insert(value_name.unwrap_or("value"), value);

        let is_error = match &validator.condition {
            Left(l) => !*l,
            Right(r) => {
                let tera_result = tera::Tera::one_off(&r, &ctx, true)?;
                let tera_result = tera_result.trim();

                validate_boolean_expression_result(&tera_result, &r)?;

                tera_result != "true"
            }
        };

        if is_error {
            let error_message = validator
                .error_message
                .as_ref()
                .map(|e| tera::Tera::one_off(&e, &ctx, true))
                .unwrap_or_else(|| {
                    Ok(format!(
                        "condition '{}' evaluated to false",
                        validator.condition
                    ))
                })?;

            return Err(ErrorInner::InvalidValue {
                prompt_name: prompt_name.to_string(),
                value: value.clone(),
                error_message,
            })?;
        }
    }

    Ok(())
}

pub fn validate_boolean_expression_result(
    result: &str,
    expression: &str,
) -> Result<(), Error> {
    if result != "true" && result != "false" {
        return Err(ErrorInner::InvalidBooleanExpressionResult {
            result: result.to_string(),
            expression: expression.to_string(),
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use value_bag::ValueBag;

    use crate::error::ErrorKind;

    use super::*;

    #[test]
    fn test_validate_value_returns_ok_if_validation_succeeds() {
        let prompt_name = "name".to_string();
        let value = ValueBag::capture_serde1(&"value").to_owned();
        let validators = [ValidateConfiguration {
            condition: Right("{{ value == 'value' }}".to_string()),
            error_message: Some("error message".to_string()),
        }];
        let ctx_vals = tera::Context::new();

        let result =
            validate_value(&prompt_name, &value, &ctx_vals, &validators, None);

        assert!(
            result.is_ok(),
            "validate_value should return ok if validation succeeds"
        );
    }

    #[test]
    fn test_validate_value_returns_error_if_validation_fails() {
        let prompt_name = "name".to_string();
        let value = ValueBag::capture_serde1(&"wrong value").to_owned();
        let validators = [ValidateConfiguration {
            condition: Right("{{ value == 'value' }}".to_string()),
            error_message: Some("error message".to_string()),
        }];
        let ctx_vals = tera::Context::new();

        let result =
            validate_value(&prompt_name, &value, &ctx_vals, &validators, None);

        assert!(
            result.is_err(),
            "validate_value should return an error if validation fails"
        );

        let err = result.unwrap_err();

        assert_eq!(
            err.kind(),
            ErrorKind::InvalidValue,
            "validate_value should return an error if validation fails"
        );
    }

    #[test]
    fn test_validate_value_returns_error_if_prompt_value_is_not_a_boolean() {
        let prompt_name = "name".to_string();
        let value = ValueBag::capture_serde1(&"wrong value").to_owned();
        let validators = [ValidateConfiguration {
            condition: Right("{{ value }}".to_string()),
            error_message: Some("error message".to_string()),
        }];
        let ctx_vals = tera::Context::new();

        let result =
            validate_value(&prompt_name, &value, &ctx_vals, &validators, None);

        assert!(
            result.is_err(),
            "validate_value should return an error if prompt value is not a boolean"
        );

        let err = result.unwrap_err();

        assert_eq!(
            err.kind(),
            ErrorKind::InvalidBooleanExpressionResult,
            "validate_value should return an error if prompt value is not a boolean"
        );
    }
}
