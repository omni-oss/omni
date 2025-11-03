use crate::configuration::{
    CheckboxPromptConfiguration, FloatNumberPromptConfiguration,
    IntegerNumberPromptConfiguration, MultiSelectPromptConfiguration,
    PasswordPromptConfiguration, PromptConfiguration,
    SelectPromptConfiguration, TextPromptConfiguration, ValidateConfiguration,
};
use derive_new::new;
use maps::UnorderedMap;
use requestty::Question;
use sets::UnorderedSet;
use strum::{EnumDiscriminants, IntoDiscriminant};
use value_bag::{OwnedValueBag, ValueBag};

pub fn prompt(
    prompts: &[PromptConfiguration],
) -> Result<UnorderedMap<String, OwnedValueBag>, PromptError> {
    validate_prompt_configurations(prompts)?;
    // TODO: conditional execution of prompts using r#if and validation of value

    let mut values = UnorderedMap::default();

    for prompt in prompts {
        let if_expr = match prompt {
            PromptConfiguration::Checkbox { prompt } => &prompt.base.r#if,
            PromptConfiguration::Select { prompt } => &prompt.base.r#if,
            PromptConfiguration::MultiSelect { prompt } => &prompt.base.r#if,
            PromptConfiguration::Text { prompt } => &prompt.base.base.r#if,
            PromptConfiguration::Password { prompt } => &prompt.base.base.r#if,
            PromptConfiguration::FloatNumber { prompt } => {
                &prompt.base.base.r#if
            }
            PromptConfiguration::IntegerNumber { prompt } => {
                &prompt.base.base.r#if
            }
        };

        if let Some(if_expr) = if_expr
            && skip(if_expr, &values)?
        {
            continue;
        }

        let (key, value, validators) = match prompt {
            PromptConfiguration::Checkbox { prompt } => {
                let value = prompt_checkbox(prompt)?;

                (
                    prompt.base.name.clone(),
                    value,
                    &[] as &[ValidateConfiguration],
                )
            }
            PromptConfiguration::Select { prompt } => {
                let value = prompt_select(prompt)?;

                (
                    prompt.base.name.clone(),
                    value,
                    &[] as &[ValidateConfiguration],
                )
            }
            PromptConfiguration::MultiSelect { prompt } => {
                let value = prompt_multi_select(prompt)?;

                (
                    prompt.base.name.clone(),
                    value,
                    &[] as &[ValidateConfiguration],
                )
            }
            PromptConfiguration::Text { prompt } => {
                let value = prompt_text(prompt)?;

                (
                    prompt.base.base.name.clone(),
                    value,
                    prompt.base.validate.as_slice(),
                )
            }
            PromptConfiguration::Password { prompt } => {
                let value = prompt_password(prompt)?;

                (
                    prompt.base.base.name.clone(),
                    value,
                    prompt.base.validate.as_slice(),
                )
            }
            PromptConfiguration::FloatNumber { prompt } => {
                let value = prompt_float_number(prompt)?;

                (
                    prompt.base.base.name.clone(),
                    value,
                    prompt.base.validate.as_slice(),
                )
            }
            PromptConfiguration::IntegerNumber { prompt } => {
                let value = prompt_integer_number(prompt)?;

                (
                    prompt.base.base.name.clone(),
                    value,
                    prompt.base.validate.as_slice(),
                )
            }
        };

        validate_value(&key, &value, validators)?;

        values.insert(key, value);
    }

    Ok(values)
}

fn skip(
    if_expr: &str,
    values: &UnorderedMap<String, OwnedValueBag>,
) -> Result<bool, PromptError> {
    let mut ctx = tera::Context::new();
    ctx.insert("prompts", values);

    let tera_result = tera::Tera::one_off(if_expr, &ctx, true)?;

    validate_boolean_expression_result(&tera_result, if_expr)?;

    // if the result is not true, then we should skip the prompt
    if tera_result != "true" {
        return Ok(true);
    }

    Ok(false)
}

fn validate_prompt_configurations(
    prompts: &[PromptConfiguration],
) -> Result<(), PromptError> {
    let mut seen_names = UnorderedSet::default();

    for prompt in prompts {
        let name = match prompt {
            PromptConfiguration::Checkbox { prompt } => &prompt.base.name,
            PromptConfiguration::Select { prompt } => &prompt.base.name,
            PromptConfiguration::MultiSelect { prompt } => &prompt.base.name,
            PromptConfiguration::Text { prompt } => &prompt.base.base.name,
            PromptConfiguration::Password { prompt } => &prompt.base.base.name,
            PromptConfiguration::FloatNumber { prompt } => {
                &prompt.base.base.name
            }
            PromptConfiguration::IntegerNumber { prompt } => {
                &prompt.base.base.name
            }
        };

        if seen_names.contains(&name) {
            return Err(PromptErrrorInner::DuplicatePromptName(name.clone()))?;
        }

        seen_names.insert(name);
    }

    Ok(())
}

fn validate_value(
    prompt_name: &str,
    value: &OwnedValueBag,
    validators: &[ValidateConfiguration],
) -> Result<(), PromptError> {
    for validator in validators {
        let mut ctx = tera::Context::new();
        ctx.insert("value", value);

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

fn prompt_checkbox(
    prompt: &CheckboxPromptConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let prompt_value = requestty::prompt_one(
        Question::confirm(prompt.base.name.as_str())
            .message(prompt.base.message.as_str())
            .build(),
    )?;

    let value = prompt_value.as_bool().ok_or_else(|| {
        PromptError::from(eyre::eyre!("prompt value is not a boolean"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_password(
    prompt: &PasswordPromptConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let prompt_value = requestty::prompt_one(
        Question::password(prompt.base.base.name.as_str())
            .message(prompt.base.base.message.as_str())
            .build(),
    )?;

    let value = prompt_value
        .as_string()
        .ok_or_else(|| {
            PromptError::from(eyre::eyre!("prompt value is not a string"))
        })?
        .to_string();

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_float_number(
    prompt: &FloatNumberPromptConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let prompt_value = requestty::prompt_one(
        Question::input(prompt.base.base.name.as_str())
            .message(prompt.base.base.message.as_str())
            .build(),
    )?;

    let value = prompt_value.as_float().ok_or_else(|| {
        PromptError::from(eyre::eyre!("prompt value is not a float"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_integer_number(
    prompt: &IntegerNumberPromptConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let prompt_value = requestty::prompt_one(
        Question::input(prompt.base.base.name.as_str())
            .message(prompt.base.base.message.as_str())
            .build(),
    )?;

    let value = prompt_value.as_int().ok_or_else(|| {
        PromptError::from(eyre::eyre!("prompt value is not an integer"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_text(
    prompt: &TextPromptConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let prompt_value = requestty::prompt_one(
        Question::input(prompt.base.base.name.as_str())
            .message(prompt.base.base.message.as_str())
            .build(),
    )?;

    let value = prompt_value
        .as_string()
        .ok_or_else(|| {
            PromptError::from(eyre::eyre!("prompt value is not a string"))
        })?
        .to_string();

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_select(
    prompt: &SelectPromptConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let options = prompt
        .options
        .iter()
        .map(|choice| choice.name.as_str())
        .collect::<Vec<_>>();

    let prompt_value = requestty::prompt_one(
        Question::select(prompt.base.name.as_str())
            .message(prompt.base.message.as_str())
            .choices(options)
            .build(),
    )?;

    let value = prompt_value
        .as_list_item()
        .map(|i| prompt.options[i.index].value.clone())
        .ok_or_else(|| {
            PromptError::from(eyre::eyre!("prompt value is not a list item"))
        })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_multi_select(
    prompt: &MultiSelectPromptConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let options = prompt
        .options
        .iter()
        .map(|choice| choice.name.as_str())
        .collect::<Vec<_>>();

    let prompt_value = requestty::prompt_one(
        Question::multi_select(prompt.base.name.as_str())
            .message(prompt.base.message.as_str())
            .choices(options)
            .build(),
    )?;

    let value = prompt_value
        .as_list_items()
        .ok_or_else(|| {
            PromptError::from(eyre::eyre!(
                "prompt value is not a list of items"
            ))
        })?
        .iter()
        .map(|i| prompt.options[i.index].value.clone())
        .collect::<Vec<_>>();

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn validate_boolean_expression_result(
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

#[derive(Debug, thiserror::Error, new)]
#[error("prompt error: {inner:?}")]
pub struct PromptError {
    #[source]
    inner: PromptErrrorInner,
    kind: PromptErrorKind,
}

impl PromptError {
    #[allow(unused)]
    pub fn kind(&self) -> PromptErrorKind {
        self.kind
    }
}

impl<T: Into<PromptErrrorInner>> From<T> for PromptError {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self {
            kind: inner.discriminant(),
            inner,
        }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(PromptErrorKind))]
enum PromptErrrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Requestty(#[from] requestty::ErrorKind),

    #[error(
        "duplicate prompt name: {0}, please ensure that all prompt names are unique"
    )]
    DuplicatePromptName(String),

    #[error(transparent)]
    Tera(#[from] tera::Error),

    #[error(
        "value '{value}' is invalid for prompt {prompt_name}: {error_message}"
    )]
    InvalidValue {
        prompt_name: String,
        value: OwnedValueBag,
        error_message: String,
    },

    #[error(
        "invalid boolean expression result: \"{result}\" for expression: \"{expression}\", expected true or false"
    )]
    InvalidBooleanExpressionResult { result: String, expression: String },
}

#[cfg(test)]
mod test {
    use crate::configuration::BasePromptConfiguration;

    use super::*;

    #[test]
    fn test_skip_returns_false_if_expression_returns_true() {
        let if_expr = "{{ prompts.name == 'value' }}";

        let values = UnorderedMap::from_iter([(
            "name".to_string(),
            ValueBag::capture_serde1(&"value").to_owned(),
        )]);

        assert_eq!(
            skip(if_expr, &values).unwrap(),
            false,
            "skip should return false"
        );
    }

    #[test]
    fn test_skip_returns_true_if_expression_returns_false() {
        let if_expr = "{{ prompts.name == 'value' }}";

        let values = UnorderedMap::from_iter([(
            "name".to_string(),
            ValueBag::capture_serde1(&"other_value").to_owned(),
        )]);

        assert_eq!(
            skip(if_expr, &values).unwrap(),
            true,
            "skip should return true"
        );
    }

    #[test]
    fn test_skip_returns_error_if_expression_returns_non_boolean_result() {
        let if_expr = "{{ prompts.name }}";

        let values = UnorderedMap::from_iter([(
            "name".to_string(),
            ValueBag::capture_serde1(&"other_value").to_owned(),
        )]);

        let result = skip(if_expr, &values);

        assert!(
            result.is_err(),
            "skip should return an error if the expression returns a non-boolean result"
        );

        let err = result.unwrap_err();

        assert_eq!(
            err.kind(),
            PromptErrorKind::InvalidBooleanExpressionResult,
            "skip should return an error if the expression returns a non-boolean result"
        );
    }

    #[test]
    fn test_validate_prompt_configurations_returns_error_if_duplicate_prompt_names()
     {
        let prompts = [
            PromptConfiguration::Checkbox {
                prompt: CheckboxPromptConfiguration {
                    base: BasePromptConfiguration {
                        name: "name".to_string(),
                        message: "message".to_string(),
                        r#if: None,
                        ..Default::default()
                    },
                },
            },
            PromptConfiguration::Checkbox {
                prompt: CheckboxPromptConfiguration {
                    base: BasePromptConfiguration {
                        name: "name".to_string(),
                        message: "message".to_string(),
                        r#if: None,
                        ..Default::default()
                    },
                },
            },
        ];

        let result = validate_prompt_configurations(&prompts);

        assert!(
            result.is_err(),
            "validate_prompt_configurations should return an error if duplicate prompt names are present"
        );

        let err = result.unwrap_err();

        assert_eq!(
            err.kind(),
            PromptErrorKind::DuplicatePromptName,
            "validate_prompt_configurations should return an error if duplicate prompt names are present"
        );
    }

    #[test]
    fn test_validate_value_returns_ok_if_validation_succeeds() {
        let prompt_name = "name".to_string();
        let value = ValueBag::capture_serde1(&"value").to_owned();
        let validators = [ValidateConfiguration {
            condition: "{{ value == 'value' }}".to_string(),
            error_message: Some("error message".to_string()),
        }];

        let result = validate_value(&prompt_name, &value, &validators);

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

        let result = validate_value(&prompt_name, &value, &validators);

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

        let result = validate_value(&prompt_name, &value, &validators);

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
