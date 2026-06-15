use either::Either::{Left, Right};
use value_bag::OwnedValueBag;

use crate::{
    configuration::ValidateConfiguration,
    error::{Error, ErrorInner},
};

pub fn validate_value(
    input_name: &str,
    value: &OwnedValueBag,
    context_values: &omni_tera::Context,
    validators: &[ValidateConfiguration],
    value_name: Option<&str>,
) -> Result<(), Error> {
    for (index, validator) in validators.iter().enumerate() {
        let mut ctx = context_values.clone();
        ctx.insert(value_name.unwrap_or("value"), value);

        let is_error = match &validator.condition {
            Left(l) => !*l,
            Right(r) => {
                let tera_result = omni_tera::one_off(
                    &r,
                    &format!(
                        "condition for input {} at index {}",
                        input_name, index
                    ),
                    &ctx,
                )?;
                let tera_result = tera_result.trim();

                validate_boolean_expression_result(&tera_result, &r)?;

                tera_result != "true"
            }
        };

        if is_error {
            let error_message = validator
                .error_message
                .as_ref()
                .map(|e| omni_tera::Tera::one_off(&e, &ctx, true))
                .unwrap_or_else(|| {
                    Ok(format!(
                        "condition '{}' evaluated to false",
                        validator.condition
                    ))
                })?;

            return Err(ErrorInner::InvalidValue {
                input_name: input_name.to_string(),
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
