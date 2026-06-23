use value_bag::OwnedValueBag;

use omni_input_schema::ValidateConfiguration;

use crate::error::Error;

pub fn validate_value(
    input_name: &str,
    value: &OwnedValueBag,
    context_values: &omni_tera::Context,
    validators: &[ValidateConfiguration],
    value_name: Option<&str>,
) -> Result<(), Error> {
    omni_input_schema::validate_value(
        input_name,
        value,
        context_values,
        validators,
        value_name,
    )
    .map_err(Into::into)
}

pub fn validate_boolean_expression_result(
    result: &str,
    expression: &str,
) -> Result<(), Error> {
    omni_input_schema::validate_boolean_expression_result(result, expression)
        .map_err(Into::into)
}
