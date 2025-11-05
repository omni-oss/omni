use std::borrow::Cow;

use requestty::Question;
use serde::Serialize;
use sets::UnorderedSet;
use value_bag::ValueBag;

use crate::{
    configuration::{
        CheckboxPromptConfiguration, FloatNumberPromptConfiguration,
        IntegerNumberPromptConfiguration, MultiSelectPromptConfiguration,
        OptionConfiguration, PasswordPromptConfiguration,
        PromptingConfiguration, SelectPromptConfiguration,
        TextPromptConfiguration, ValidateConfiguration,
    },
    error::{Error, ErrorInner},
    utils::validate_value,
};

pub fn checkbox<'a>(
    prompt: &'a CheckboxPromptConfiguration,
    _context_values: &'a tera::Context,
    _config: &'a PromptingConfiguration,
) -> Result<requestty::Question<'a>, Error> {
    let name = prompt.base.name.as_str();
    let default_value = prompt.default;

    let question =
        Question::confirm(name).message(prompt.base.message.as_str());

    Ok(if let Some(default_value) = default_value {
        question.default(default_value)
    } else {
        question
    }
    .build())
}

pub fn password<'a>(
    prompt: &'a PasswordPromptConfiguration,
    context_values: &'a tera::Context,
    config: &'a PromptingConfiguration,
) -> Result<requestty::Question<'a>, Error> {
    let name = prompt.base.base.name.as_str();
    let validators = prompt.base.validate.as_slice();

    let question = Question::password(name)
        .message(prompt.base.base.message.as_str())
        .validate(|answer, _| {
            validate(
                &answer.to_string(),
                name,
                context_values,
                validators,
                config,
            )
        });

    Ok(question.build())
}

pub fn text<'a>(
    prompt: &'a TextPromptConfiguration,
    context_values: &'a tera::Context,
    config: &'a PromptingConfiguration,
) -> Result<requestty::Question<'a>, Error> {
    let name = prompt.base.base.name.as_str();
    let validators = prompt.base.validate.as_slice();
    let question = Question::input(name)
        .message(prompt.base.base.message.as_str())
        .validate(|answer, _| {
            validate(
                &answer.to_string(),
                name,
                context_values,
                validators,
                config,
            )
        });
    let default_value = prompt.default.as_deref();

    Ok(if let Some(default_value) = default_value {
        question.default(default_value)
    } else {
        question
    }
    .build())
}

pub fn select<'a>(
    prompt: &'a SelectPromptConfiguration,
    _context_values: &'a tera::Context,
    _config: &'a PromptingConfiguration,
) -> Result<requestty::Question<'a>, Error> {
    let default_value = prompt.default.as_deref();
    let mut question = Question::select(prompt.base.name.as_str())
        .message(prompt.base.message.as_str());

    for option in prompt.options.iter() {
        let text = get_option_text(option);
        if option.separator {
            question = question.separator(text);
        } else {
            question = question.choice(text);
        }
    }

    Ok(if let Some(default) = default_value
        && let Some(index) =
            prompt.options.iter().position(|o| o.value == default)
    {
        question.default(index)
    } else {
        question
    }
    .build())
}

pub fn multi_select<'a>(
    prompt: &'a MultiSelectPromptConfiguration,
    context_values: &'a tera::Context,
    config: &'a PromptingConfiguration,
) -> Result<requestty::Question<'a>, Error> {
    let name = prompt.base.base.name.as_str();
    let default_values = prompt
        .default
        .as_deref()
        .map(|def| def.iter().collect::<UnorderedSet<_>>());
    let validators = prompt.base.validate.as_slice();

    let mut question = Question::multi_select(name)
        .message(prompt.base.base.message.as_str())
        .validate(|answers, _| {
            let values = answers
                .iter()
                .enumerate()
                .filter_map(|(i, value)| {
                    if !value {
                        return None;
                    }

                    prompt.options.get(i).map(|o| o.value.clone())
                })
                .collect::<Vec<_>>();

            validate(&values, name, context_values, validators, config)
        });

    if let Some(defaults) = default_values {
        for option in prompt.options.iter() {
            let text = get_option_text(option);
            if option.separator {
                question = question.separator(text);
            } else {
                question = question.choice_with_default(
                    text,
                    defaults.contains(&option.value),
                );
            }
        }
    } else {
        for option in prompt.options.iter() {
            let text = get_option_text(option);
            if option.separator {
                question = question.separator(text);
            } else {
                question = question.choice(text);
            }
        }
    };

    Ok(question.build())
}

fn get_option_text<'a>(option: &'a OptionConfiguration) -> Cow<'a, str> {
    if option.separator {
        return Cow::Borrowed(&option.name);
    }

    if let Some(description) = &option.description {
        Cow::Owned(format!(
            "{} - {} ({})",
            option.name, description, option.value
        ))
    } else if option.name != option.value {
        Cow::Owned(format!("{} ({})", option.name, option.value))
    } else {
        Cow::Borrowed(&option.name)
    }
}

pub fn float_number<'a>(
    prompt: &'a FloatNumberPromptConfiguration,
    context_values: &'a tera::Context,
    config: &'a PromptingConfiguration,
) -> Result<requestty::Question<'a>, Error> {
    let name = prompt.base.base.name.as_str();
    let validators = prompt.base.validate.as_slice();
    let question = Question::input(name)
        .message(prompt.base.base.message.as_str())
        .validate(|answer, _| {
            if let Ok(value) = answer.parse::<f64>() {
                validate(&value, name, context_values, validators, config)
            } else {
                Err("value is not an integer".to_string())
            }
        });
    let default_value = prompt.default;

    Ok(if let Some(default_value) = default_value {
        question.default(default_value.to_string())
    } else {
        question
    }
    .build())
}

pub fn integer_number<'a>(
    prompt: &'a IntegerNumberPromptConfiguration,
    context_values: &'a tera::Context,
    config: &'a PromptingConfiguration,
) -> Result<requestty::Question<'a>, Error> {
    let name = prompt.base.base.name.as_str();
    let validators = prompt.base.validate.as_slice();
    let question = Question::input(name)
        .message(prompt.base.base.message.as_str())
        .validate(|answer, _| {
            if let Ok(value) = answer.parse::<i64>() {
                validate(&value, name, context_values, validators, config)
            } else {
                Err("value is not an integer".to_string())
            }
        });
    let default_value = prompt.default;

    Ok(if let Some(default_value) = default_value {
        question.default(default_value.to_string())
    } else {
        question
    }
    .build())
}

fn validate<T: Serialize + 'static>(
    value: &T,
    name: &str,
    context_values: &tera::Context,
    validators: &[ValidateConfiguration],
    config: &PromptingConfiguration,
) -> Result<(), String> {
    let value = ValueBag::capture_serde1(value).to_owned();

    let result = validate_value(
        name,
        &value,
        context_values,
        validators,
        config.validation_expressions_value_name,
    );

    if let Err(err) = result {
        if let ErrorInner::InvalidValue { error_message, .. } = &err.inner {
            Err(error_message.to_string())
        } else {
            // TODO: find a better way to show errors,
            // currently this is badly displayed due to prompt formatting in the terminal
            trace::error!("{:?}", err);
            std::process::exit(1);
        }
    } else {
        Ok(())
    }
}
