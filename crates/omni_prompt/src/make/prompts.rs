use std::{borrow::Cow, str::FromStr};

use either::Either;
use omni_input_provider::{
    configuration::{
        ConfirmInputConfiguration, FloatInputConfiguration,
        IntegerInputConfiguration, MultiSelectInputConfiguration,
        OptionConfiguration, PasswordInputConfiguration,
        SelectInputConfiguration, TextInputConfiguration,
        ValidateConfiguration,
    },
    error::{Error, ErrorInner},
    utils::validate_value,
};
use requestty::Question;
use serde::Serialize;
use sets::unordered_set;
use value_bag::ValueBag;

pub fn confirm<'a>(
    input: &'a ConfirmInputConfiguration,
    context_values: &'a omni_tera::Context,
) -> Result<requestty::Question<'a>, Error> {
    let name = input.base.name.as_str();
    let default_value = &input.default;

    let question = Question::confirm(name).message(input.base.message.as_str());

    Ok(if let Some(default_value) = default_value {
        question.default(try_parse_or_expand_default_value(
            name,
            "bool",
            &default_value,
            context_values,
        )?)
    } else {
        question
    }
    .build())
}

pub fn password<'a>(
    input: &'a PasswordInputConfiguration,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<requestty::Question<'a>, Error> {
    let name = input.base.base.name.as_str();
    let validators = input.base.validate.as_slice();

    let question = Question::password(name)
        .message(input.base.base.message.as_str())
        .validate(move |answer, _| {
            validate(
                &answer.to_string(),
                name,
                context_values,
                validators,
                validation_value_name,
            )
        });

    Ok(question.build())
}

pub fn text<'a>(
    input: &'a TextInputConfiguration,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<requestty::Question<'a>, Error> {
    let name = input.base.base.name.as_str();
    let validators = input.base.validate.as_slice();
    let question = Question::input(name)
        .message(input.base.base.message.as_str())
        .validate(move |answer, _| {
            validate(
                &answer.to_string(),
                name,
                context_values,
                validators,
                validation_value_name,
            )
        });
    let default_value = input
        .default
        .as_deref()
        .map(|v| expand_default_value(v, name, context_values))
        .transpose()?;

    Ok(if let Some(default_value) = default_value {
        question.default(default_value)
    } else {
        question
    }
    .build())
}

pub fn select<'a>(
    input: &'a SelectInputConfiguration,
    context_values: &'a omni_tera::Context,
) -> Result<requestty::Question<'a>, Error> {
    let name = input.base.name.as_str();
    let default_value = input
        .default
        .as_deref()
        .map(|v| expand_default_value(v, name, context_values))
        .transpose()?;

    let mut question =
        Question::select(name).message(input.base.message.as_str());

    for option in input.options.iter() {
        let text = get_option_text(option);
        if option.separator {
            question = question.separator(text);
        } else {
            question = question.choice(text);
        }
    }

    Ok(if let Some(default) = default_value
        && let Some(index) =
            input.options.iter().position(|o| o.value == default)
    {
        question.default(index)
    } else {
        question
    }
    .build())
}

pub fn multi_select<'a>(
    input: &'a MultiSelectInputConfiguration,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<requestty::Question<'a>, Error> {
    let name = input.base.base.name.as_str();
    let default_values = if let Some(default_values) = &input.default {
        let mut values = unordered_set!();
        for option in default_values {
            values.insert(expand_default_value(option, name, context_values)?);
        }
        Some(values)
    } else {
        None
    };

    let validators = input.base.validate.as_slice();

    let mut question = Question::multi_select(name)
        .message(input.base.base.message.as_str())
        .validate(move |answers, _| {
            let values = answers
                .iter()
                .enumerate()
                .filter_map(|(i, value)| {
                    if !value {
                        return None;
                    }
                    input.options.get(i).map(|o| o.value.clone())
                })
                .collect::<Vec<_>>();
            validate(
                &values,
                name,
                context_values,
                validators,
                validation_value_name,
            )
        });

    if let Some(defaults) = default_values {
        for option in input.options.iter() {
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
        for option in input.options.iter() {
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
    input: &'a FloatInputConfiguration,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<requestty::Question<'a>, Error> {
    let name = input.base.base.name.as_str();
    let validators = input.base.validate.as_slice();
    let question = Question::input(name)
        .message(input.base.base.message.as_str())
        .validate(move |answer, _| {
            if let Ok(value) = answer.parse::<f64>() {
                validate(
                    &value,
                    name,
                    context_values,
                    validators,
                    validation_value_name,
                )
            } else {
                Err("value is not a float".to_string())
            }
        });
    let default_value = input
        .default
        .as_ref()
        .map(|v| {
            try_parse_or_expand_default_value::<f64>(
                name,
                "float",
                v,
                context_values,
            )
        })
        .transpose()?;

    Ok(if let Some(default_value) = default_value {
        question.default(default_value.to_string())
    } else {
        question
    }
    .build())
}

pub fn integer_number<'a>(
    input: &'a IntegerInputConfiguration,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<requestty::Question<'a>, Error> {
    let name = input.base.base.name.as_str();
    let validators = input.base.validate.as_slice();
    let question = Question::input(name)
        .message(input.base.base.message.as_str())
        .validate(move |answer, _| {
            if let Ok(value) = answer.parse::<i64>() {
                validate(
                    &value,
                    name,
                    context_values,
                    validators,
                    validation_value_name,
                )
            } else {
                Err("value is not an integer".to_string())
            }
        });
    let default_value = input
        .default
        .as_ref()
        .map(|v| {
            try_parse_or_expand_default_value::<i64>(
                name,
                "integer",
                v,
                context_values,
            )
        })
        .transpose()?;

    Ok(if let Some(default_value) = default_value {
        question.default(default_value.to_string())
    } else {
        question
    }
    .build())
}

fn try_parse_or_expand_default_value<T: FromStr + Clone>(
    input_name: &str,
    expected_type: &str,
    value: &Either<T, String>,
    context_values: &omni_tera::Context,
) -> Result<T, Error> {
    match value {
        Either::Left(value) => Ok(value.clone()),
        Either::Right(value) => {
            if let Ok(value) = value.parse::<T>() {
                Ok(value)
            } else {
                let expanded = omni_tera::one_off(
                    &value,
                    &format!("default value for input {}", input_name),
                    context_values,
                )?;
                let parsed = expanded.as_str().parse::<T>();

                if let Ok(value) = parsed {
                    Ok(value)
                } else {
                    Err(Error::from(
                        omni_input_provider::error::ErrorInner::InvalidValue {
                            input_name: input_name.to_string(),
                            value: ValueBag::capture_serde1(&expanded)
                                .to_owned(),
                            error_message: format!(
                                "Value '{}' is not a valid {} value",
                                value, expected_type
                            ),
                        },
                    ))
                }
            }
        }
    }
}

fn expand_default_value(
    template: &str,
    name: &str,
    context_values: &omni_tera::Context,
) -> Result<String, Error> {
    let expanded = omni_tera::one_off(
        template,
        &format!("default value for input {}", name),
        context_values,
    )?;
    Ok(expanded)
}

fn validate<T: Serialize + 'static>(
    value: &T,
    name: &str,
    context_values: &omni_tera::Context,
    validators: &[ValidateConfiguration],
    validation_value_name: Option<&str>,
) -> Result<(), String> {
    let value = ValueBag::capture_serde1(value).to_owned();

    let result = validate_value(
        name,
        &value,
        context_values,
        validators,
        validation_value_name,
    );

    if let Err(err) = result {
        if let ErrorInner::InvalidValue { error_message, .. } = &err.0 {
            Err(error_message.to_string())
        } else {
            // Return a descriptive error string instead of exiting the process.
            Err(format!("unexpected validation error: {err:?}"))
        }
    } else {
        Ok(())
    }
}
