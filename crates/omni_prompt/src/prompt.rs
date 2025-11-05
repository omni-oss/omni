use crate::{
    configuration::{
        CheckboxPromptConfiguration, FloatNumberPromptConfiguration,
        IntegerNumberPromptConfiguration, MultiSelectPromptConfiguration,
        PasswordPromptConfiguration, PromptConfiguration,
        PromptingConfiguration, SelectPromptConfiguration,
        TextPromptConfiguration, ValidateConfiguration,
    },
    error::{Error, ErrorInner, ErrorKind},
    make,
    utils::{validate_boolean_expression_result, validate_value},
};
use maps::{UnorderedMap, unordered_map};
use sets::UnorderedSet;
use value_bag::{OwnedValueBag, ValueBag};

pub fn prompt(
    prompts: &[PromptConfiguration],
    pre_exec_values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &PromptingConfiguration,
) -> Result<UnorderedMap<String, OwnedValueBag>, Error> {
    validate_prompt_configurations(prompts)?;
    let mut values = UnorderedMap::default();

    let mut ctx_vals = tera::Context::new();

    for (key, value) in context_values {
        ctx_vals.insert(key, value);
    }

    for prompt in prompts {
        let if_expr = get_if_expression(prompt);

        if let Some(if_expr) = if_expr
            && skip(
                if_expr,
                &values,
                &ctx_vals,
                config.if_expressions_root_property,
            )?
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
                &ctx_vals,
                validators,
                config.validation_expressions_value_name,
            );

            if let Err(err) = result {
                if err.kind() == ErrorKind::InvalidValue {
                    trace::error!("reprompting due to error: {err}");
                    get_prompt_value(prompt, &ctx_vals, config)?
                } else {
                    return Err(err);
                }
            } else {
                value.clone()
            }
        } else {
            get_prompt_value(prompt, &ctx_vals, config)?
        };

        values.insert(key, value);
    }

    Ok(values)
}

pub fn prompt_one(
    prompt: &PromptConfiguration,
    pre_exec_value: Option<&OwnedValueBag>,
    context_values: &UnorderedMap<String, OwnedValueBag>,
    config: &PromptingConfiguration,
) -> Result<Option<OwnedValueBag>, Error> {
    let mut ctx_vals = tera::Context::new();

    for (key, value) in context_values {
        ctx_vals.insert(key, value);
    }

    let if_expr = get_if_expression(prompt);

    let mut pre_exec_values = unordered_map!();

    if let Some(pre_exec_value) = pre_exec_value {
        pre_exec_values.insert(
            get_prompt_name(prompt).to_string(),
            pre_exec_value.clone(),
        );
    }

    if let Some(if_expr) = if_expr
        && skip(
            if_expr,
            &pre_exec_values,
            &ctx_vals,
            config.if_expressions_root_property,
        )?
    {
        return Ok(None);
    }

    let validators = get_validators(prompt);
    let key = get_prompt_name(prompt).to_string();
    let value = if let Some(value) = pre_exec_value {
        let result = validate_value(
            &key,
            &value,
            &ctx_vals,
            validators,
            config.validation_expressions_value_name,
        );

        if let Err(err) = result {
            if err.kind() == ErrorKind::InvalidValue {
                trace::error!("reprompting due to error: {err}");
                get_prompt_value(prompt, &ctx_vals, config)?
            } else {
                return Err(err);
            }
        } else {
            value.clone()
        }
    } else {
        get_prompt_value(prompt, &ctx_vals, config)?
    };

    Ok(Some(value))
}

fn get_prompt_value(
    prompt: &PromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration<'_>,
) -> Result<OwnedValueBag, Error> {
    let value = match prompt {
        PromptConfiguration::Checkbox { prompt } => {
            prompt_checkbox(prompt, context_values, config)?
        }
        PromptConfiguration::Select { prompt } => {
            prompt_select(prompt, context_values, config)?
        }
        PromptConfiguration::MultiSelect { prompt } => {
            prompt_multi_select(prompt, context_values, config)?
        }
        PromptConfiguration::Text { prompt } => {
            prompt_text(prompt, context_values, config)?
        }
        PromptConfiguration::Password { prompt } => {
            prompt_password(prompt, context_values, config)?
        }
        PromptConfiguration::Float { prompt } => {
            prompt_float_number(prompt, context_values, config)?
        }
        PromptConfiguration::Integer { prompt } => {
            prompt_integer_number(prompt, context_values, config)?
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
        PromptConfiguration::Float { prompt } => &prompt.base.validate,
        PromptConfiguration::Integer { prompt } => &prompt.base.validate,
    }
}

fn get_if_expression(prompt: &PromptConfiguration) -> Option<&str> {
    let if_expr = match prompt {
        PromptConfiguration::Checkbox { prompt } => &prompt.base.r#if,
        PromptConfiguration::Select { prompt } => &prompt.base.r#if,
        PromptConfiguration::MultiSelect { prompt } => &prompt.base.base.r#if,
        PromptConfiguration::Text { prompt } => &prompt.base.base.r#if,
        PromptConfiguration::Password { prompt } => &prompt.base.base.r#if,
        PromptConfiguration::Float { prompt } => &prompt.base.base.r#if,
        PromptConfiguration::Integer { prompt } => &prompt.base.base.r#if,
    };
    if_expr.as_deref()
}

fn skip(
    if_expr: &str,
    values: &UnorderedMap<String, OwnedValueBag>,
    context_values: &tera::Context,
    if_expressions_root_property: Option<&str>,
) -> Result<bool, Error> {
    let mut ctx = context_values.clone();
    ctx.insert(if_expressions_root_property.unwrap_or("prompts"), values);

    let tera_result = tera::Tera::one_off(if_expr, &ctx, true)?;
    let tera_result = tera_result.trim();

    validate_boolean_expression_result(&tera_result, if_expr)?;

    // if the result is not true, then we should skip the prompt
    if tera_result != "true" {
        return Ok(true);
    }

    Ok(false)
}

fn validate_prompt_configurations(
    prompts: &[PromptConfiguration],
) -> Result<(), Error> {
    let mut seen_names = UnorderedSet::default();

    for prompt in prompts {
        let name = get_prompt_name(prompt);

        if seen_names.contains(&name) {
            return Err(ErrorInner::DuplicatePromptName(name.to_string()))?;
        }

        seen_names.insert(name);
    }

    Ok(())
}

fn get_prompt_name(prompt: &PromptConfiguration) -> &str {
    let name = match prompt {
        PromptConfiguration::Checkbox { prompt } => &prompt.base.name,
        PromptConfiguration::Select { prompt } => &prompt.base.name,
        PromptConfiguration::MultiSelect { prompt } => &prompt.base.base.name,
        PromptConfiguration::Text { prompt } => &prompt.base.base.name,
        PromptConfiguration::Password { prompt } => &prompt.base.base.name,
        PromptConfiguration::Float { prompt } => &prompt.base.base.name,
        PromptConfiguration::Integer { prompt } => &prompt.base.base.name,
    };
    name
}

fn prompt_checkbox(
    prompt: &CheckboxPromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration,
) -> Result<OwnedValueBag, Error> {
    let prompt = make::checkbox(prompt, context_values, config)?;

    let prompt_value = requestty::prompt_one(prompt)?;

    let value = prompt_value.as_bool().ok_or_else(|| {
        Error::from(eyre::eyre!("prompt value is not a boolean"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_password(
    prompt: &PasswordPromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration,
) -> Result<OwnedValueBag, Error> {
    let question = make::password(prompt, context_values, config)?;

    let prompt_value = requestty::prompt_one(question)?;

    let value = prompt_value
        .as_string()
        .ok_or_else(|| {
            Error::from(eyre::eyre!("prompt value is not a string"))
        })?
        .to_string();

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_float_number(
    prompt: &FloatNumberPromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration,
) -> Result<OwnedValueBag, Error> {
    let question = make::float_number(prompt, context_values, config)?;

    let prompt_value = requestty::prompt_one(question)?;

    let value = prompt_value.as_float().ok_or_else(|| {
        Error::from(eyre::eyre!("prompt value is not a float"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_integer_number(
    prompt: &IntegerNumberPromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration,
) -> Result<OwnedValueBag, Error> {
    let question = make::integer_number(prompt, context_values, config)?;

    let prompt_value = requestty::prompt_one(question)?;

    let value = prompt_value.as_int().ok_or_else(|| {
        Error::from(eyre::eyre!("prompt value is not an integer"))
    })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_text(
    prompt: &TextPromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration,
) -> Result<OwnedValueBag, Error> {
    let question = make::text(prompt, context_values, config)?;

    let prompt_value = requestty::prompt_one(question)?;

    let value = prompt_value
        .as_string()
        .ok_or_else(|| {
            Error::from(eyre::eyre!("prompt value is not a string"))
        })?
        .to_string();

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_select(
    prompt: &SelectPromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration,
) -> Result<OwnedValueBag, Error> {
    let question = make::select(prompt, context_values, config)?;

    let prompt_value = requestty::prompt_one(question)?;

    let value = prompt_value
        .as_list_item()
        .map(|i| prompt.options[i.index].value.clone())
        .ok_or_else(|| {
            Error::from(eyre::eyre!("prompt value is not a list item"))
        })?;

    Ok(ValueBag::capture_serde1(&value).to_owned())
}

fn prompt_multi_select(
    prompt: &MultiSelectPromptConfiguration,
    context_values: &tera::Context,
    config: &PromptingConfiguration,
) -> Result<OwnedValueBag, Error> {
    let question = make::multi_select(prompt, context_values, config)?;

    let prompt_value = requestty::prompt_one(question)?;

    let value = prompt_value
        .as_list_items()
        .ok_or_else(|| {
            Error::from(eyre::eyre!("prompt value is not a list of items"))
        })?
        .iter()
        .map(|i| prompt.options[i.index].value.clone())
        .collect::<Vec<_>>();

    Ok(ValueBag::capture_serde1(&value).to_owned())
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
        let ctx_vals = tera::Context::new();

        assert_eq!(
            skip(if_expr, &values, &ctx_vals, None).unwrap(),
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
        let ctx_vals = tera::Context::new();

        assert_eq!(
            skip(if_expr, &values, &ctx_vals, None).unwrap(),
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
        let ctx_vals = tera::Context::new();

        let result = skip(if_expr, &values, &ctx_vals, None);

        assert!(
            result.is_err(),
            "skip should return an error if the expression returns a non-boolean result"
        );

        let err = result.unwrap_err();

        assert_eq!(
            err.kind(),
            ErrorKind::InvalidBooleanExpressionResult,
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
            ErrorKind::DuplicatePromptName,
            "validate_prompt_configurations should return an error if duplicate prompt names are present"
        );
    }
}
