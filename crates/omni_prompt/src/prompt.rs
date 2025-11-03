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

#[derive(Debug, new)]
pub struct PromptingConfiguration<'a> {
    pub if_expressions_root_property: Option<&'a str>,
    pub validation_expressions_value_name: Option<&'a str>,
}

impl<'a> Default for PromptingConfiguration<'a> {
    fn default() -> Self {
        Self {
            if_expressions_root_property: Some("prompts"),
            validation_expressions_value_name: Some("value"),
        }
    }
}

pub fn prompt(
    prompts: &[PromptConfiguration],
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    config: &PromptingConfiguration,
) -> Result<UnorderedMap<String, OwnedValueBag>, PromptError> {
    validate_prompt_configurations(prompts)?;
    let mut values = UnorderedMap::default();

    for prompt in prompts {
        let if_expr = get_if_expression(prompt);

        if let Some(if_expr) = if_expr
            && skip(if_expr, &values, config.if_expressions_root_property)?
        {
            continue;
        }

        let validators = get_validators(prompt);
        let key = get_prompt_name(prompt).to_string();
        let pre_exec_value = pre_exec_values.get(&key);
        let value = if let Some(pre_exec_value) = pre_exec_value {
            let value = pre_exec_value;
            let result = validate_value(
                &key,
                &value,
                validators,
                config.validation_expressions_value_name,
            );

            if let Err(err) = result {
                if err.kind() == PromptErrorKind::InvalidValue {
                    trace::error!("reprompting due to error: {err}");
                    get_prompt_value(prompt, config)?
                } else {
                    return Err(err);
                }
            } else {
                value.clone()
            }
        } else {
            get_prompt_value(prompt, config)?
        };

        values.insert(key, value);
    }

    Ok(values)
}

fn get_prompt_value(
    prompt: &PromptConfiguration,
    config: &PromptingConfiguration<'_>,
) -> Result<OwnedValueBag, PromptError> {
    let value = match prompt {
        PromptConfiguration::Checkbox { prompt } => {
            prompt_checkbox(prompt, config)?
        }
        PromptConfiguration::Select { prompt } => {
            prompt_select(prompt, config)?
        }
        PromptConfiguration::MultiSelect { prompt } => {
            prompt_multi_select(prompt, config)?
        }
        PromptConfiguration::Text { prompt } => prompt_text(prompt, config)?,
        PromptConfiguration::Password { prompt } => {
            prompt_password(prompt, config)?
        }
        PromptConfiguration::FloatNumber { prompt } => {
            prompt_float_number(prompt, config)?
        }
        PromptConfiguration::IntegerNumber { prompt } => {
            prompt_integer_number(prompt, config)?
        }
    };
    Ok(value)
}

fn get_validators(prompt: &PromptConfiguration) -> &[ValidateConfiguration] {
    match prompt {
        PromptConfiguration::Checkbox { .. } => &[],
        PromptConfiguration::Select { .. } => &[],
        PromptConfiguration::MultiSelect { .. } => &[],
        PromptConfiguration::Text { prompt } => &prompt.base.validate,
        PromptConfiguration::Password { prompt } => &prompt.base.validate,
        PromptConfiguration::FloatNumber { prompt } => &prompt.base.validate,
        PromptConfiguration::IntegerNumber { prompt } => &prompt.base.validate,
    }
}

fn get_if_expression(prompt: &PromptConfiguration) -> Option<&str> {
    let if_expr = match prompt {
        PromptConfiguration::Checkbox { prompt } => &prompt.base.r#if,
        PromptConfiguration::Select { prompt } => &prompt.base.r#if,
        PromptConfiguration::MultiSelect { prompt } => &prompt.base.r#if,
        PromptConfiguration::Text { prompt } => &prompt.base.base.r#if,
        PromptConfiguration::Password { prompt } => &prompt.base.base.r#if,
        PromptConfiguration::FloatNumber { prompt } => &prompt.base.base.r#if,
        PromptConfiguration::IntegerNumber { prompt } => &prompt.base.base.r#if,
    };
    if_expr.as_deref()
}

fn skip(
    if_expr: &str,
    values: &UnorderedMap<String, OwnedValueBag>,
    if_expressions_root_property: Option<&str>,
) -> Result<bool, PromptError> {
    let mut ctx = tera::Context::new();
    ctx.insert(if_expressions_root_property.unwrap_or("prompts"), values);

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
        let name = get_prompt_name(prompt);

        if seen_names.contains(&name) {
            return Err(PromptErrrorInner::DuplicatePromptName(
                name.to_string(),
            ))?;
        }

        seen_names.insert(name);
    }

    Ok(())
}

fn get_prompt_name(prompt: &PromptConfiguration) -> &str {
    let name = match prompt {
        PromptConfiguration::Checkbox { prompt } => &prompt.base.name,
        PromptConfiguration::Select { prompt } => &prompt.base.name,
        PromptConfiguration::MultiSelect { prompt } => &prompt.base.name,
        PromptConfiguration::Text { prompt } => &prompt.base.base.name,
        PromptConfiguration::Password { prompt } => &prompt.base.base.name,
        PromptConfiguration::FloatNumber { prompt } => &prompt.base.base.name,
        PromptConfiguration::IntegerNumber { prompt } => &prompt.base.base.name,
    };
    name
}

fn validate_value(
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

// TODO: utilize requestty's validate feature

fn prompt_checkbox(
    prompt: &CheckboxPromptConfiguration,
    _config: &PromptingConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let name = prompt.base.name.as_str();
    let default_value = prompt.default;

    let question =
        Question::confirm(name).message(prompt.base.message.as_str());

    let prompt_value = requestty::prompt_one(
        if let Some(default_value) = default_value {
            question.default(default_value)
        } else {
            question
        }
        .build(),
    )?;

    let value = prompt_value.as_bool().ok_or_else(|| {
        PromptError::from(eyre::eyre!("prompt value is not a boolean"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_password(
    prompt: &PasswordPromptConfiguration,
    _config: &PromptingConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let name = prompt.base.base.name.as_str();

    let question =
        Question::password(name).message(prompt.base.base.message.as_str());

    let prompt_value = requestty::prompt_one(question.build())?;

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
    _config: &PromptingConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let name = prompt.base.base.name.as_str();
    let question =
        Question::input(name).message(prompt.base.base.message.as_str());
    let default_value = prompt.default;

    let prompt_value = requestty::prompt_one(
        if let Some(default_value) = default_value {
            question.default(default_value.to_string())
        } else {
            question
        }
        .build(),
    )?;

    let value = prompt_value.as_float().ok_or_else(|| {
        PromptError::from(eyre::eyre!("prompt value is not a float"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_integer_number(
    prompt: &IntegerNumberPromptConfiguration,
    _config: &PromptingConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let name = prompt.base.base.name.as_str();
    let question =
        Question::input(name).message(prompt.base.base.message.as_str());
    let default_value = prompt.default;

    let prompt_value = requestty::prompt_one(
        if let Some(default_value) = default_value {
            question.default(default_value.to_string())
        } else {
            question
        }
        .build(),
    )?;

    let value = prompt_value.as_int().ok_or_else(|| {
        PromptError::from(eyre::eyre!("prompt value is not an integer"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_text(
    prompt: &TextPromptConfiguration,
    _config: &PromptingConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let name = prompt.base.base.name.as_str();
    let question =
        Question::input(name).message(prompt.base.base.message.as_str());
    let default_value = prompt.default.as_deref();

    let prompt_value = requestty::prompt_one(
        if let Some(default_value) = default_value {
            question.default(default_value)
        } else {
            question
        }
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
    _config: &PromptingConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let options = prompt
        .options
        .iter()
        .map(|choice| choice.name.as_str())
        .collect::<Vec<_>>();

    let default_value = prompt.default.as_deref();
    let question = Question::select(prompt.base.name.as_str())
        .message(prompt.base.message.as_str())
        .choices(options);

    let prompt_value = requestty::prompt_one(
        if let Some(default) = default_value
            && let Some(index) =
                prompt.options.iter().position(|o| o.value == default)
        {
            question.default(index)
        } else {
            question
        }
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
    _config: &PromptingConfiguration,
) -> Result<OwnedValueBag, PromptError> {
    let name = prompt.base.name.as_str();
    let default_values = prompt
        .default
        .as_deref()
        .map(|def| def.iter().collect::<UnorderedSet<_>>());

    let question =
        Question::multi_select(name).message(prompt.base.message.as_str());

    let prompt_value = requestty::prompt_one(
        if let Some(defaults) = default_values {
            let options = prompt
                .options
                .iter()
                .map(|choice| {
                    (choice.name.as_str(), defaults.contains(&choice.value))
                })
                .collect::<Vec<_>>();

            question.choices_with_default(options)
        } else {
            let options = prompt
                .options
                .iter()
                .map(|choice| choice.name.as_str())
                .collect::<Vec<_>>();
            question.choices(options)
        }
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
            skip(if_expr, &values, None).unwrap(),
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
            skip(if_expr, &values, None).unwrap(),
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

        let result = skip(if_expr, &values, None);

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
                    default: None,
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
                    default: None,
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
