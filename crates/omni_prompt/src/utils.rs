use value_bag::OwnedValueBag;

use crate::{
    configuration::ValidateConfiguration,
    error::{PromptError, PromptErrrorInner},
};

pub fn validate_value(
    prompt_name: &str,
    value: &OwnedValueBag,
    validators: &[ValidateConfiguration],
    value_name: Option<&str>,
) -> Result<(), PromptError> {
    for validator in validators {
        let mut ctx = tera::Context::new();
        ctx.insert(value_name.unwrap_or("value"), value);

        let tera_result =
            tera::Tera::one_off(&validator.condition, &ctx, true)?;

        validate_boolean_expression_result(&tera_result, &validator.condition)?;

        if tera_result != "true" {
            return Err(PromptErrrorInner::InvalidValue {
                prompt_name: prompt_name.to_string(),
                value: value.clone(),
                error_message: validator.error_message.clone().unwrap_or_else(
                    || {
                        format!(
                            "condition '{}' evaluated to false",
                            validator.condition
                        )
                    },
                ),
            })?;
        }
    }

    Ok(())
}

pub fn validate_boolean_expression_result(
    result: &str,
    expression: &str,
) -> Result<(), PromptError> {
    if result != "true" && result != "false" {
        return Err(PromptErrrorInner::InvalidBooleanExpressionResult {
            result: result.to_string(),
            expression: expression.to_string(),
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use value_bag::ValueBag;

    use crate::error::PromptErrorKind;

    use super::*;

    #[test]
    fn test_validate_value_returns_ok_if_validation_succeeds() {
        let prompt_name = "name".to_string();
        let value = ValueBag::capture_serde1(&"value").to_owned();
        let validators = [ValidateConfiguration {
            condition: "{{ value == 'value' }}".to_string(),
            error_message: Some("error message".to_string()),
        }];

        let result = validate_value(&prompt_name, &value, &validators, None);

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
            condition: "{{ value == 'value' }}".to_string(),
            error_message: Some("error message".to_string()),
        }];

        let result = validate_value(&prompt_name, &value, &validators, None);

        assert!(
            result.is_err(),
            "validate_value should return an error if validation fails"
        );

        let err = result.unwrap_err();

        assert_eq!(
            err.kind(),
            PromptErrorKind::InvalidValue,
            "validate_value should return an error if validation fails"
        );
    }

    #[test]
    fn test_validate_value_returns_error_if_prompt_value_is_not_a_boolean() {
        let prompt_name = "name".to_string();
        let value = ValueBag::capture_serde1(&"wrong value").to_owned();
        let validators = [ValidateConfiguration {
            condition: "{{ value }}".to_string(),
            error_message: Some("error message".to_string()),
        }];

        let result = validate_value(&prompt_name, &value, &validators, None);

        assert!(
            result.is_err(),
            "validate_value should return an error if prompt value is not a boolean"
        );

        let err = result.unwrap_err();

        assert_eq!(
            err.kind(),
            PromptErrorKind::InvalidBooleanExpressionResult,
            "validate_value should return an error if prompt value is not a boolean"
        );
    }
}
