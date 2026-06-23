use std::borrow::Cow;

use omni_generator_configurations::Generator;
use omni_input_provider::{
    AllowedValue, BooleanInput, FloatArrayInput, FloatInput, IntegerArrayInput,
    IntegerInput, StringArrayInput, StringInput, ValidateConfiguration,
    error::{Error, ErrorInner},
    utils::validate_value,
};
use requestty::Question;
use serde::Serialize;
use sets::unordered_set;
use value_bag::ValueBag;

/// Build a `confirm` question for a boolean input.
pub fn confirm<'a>(
    input: &'a BooleanInput<Generator>,
    _context_values: &'a omni_tera::Context,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let question =
        Question::confirm(name).message(input.base_extra.message.as_str());
    Ok(if let Some(default_value) = input.default {
        question.default(default_value)
    } else {
        question
    }
    .build())
}

/// Build a `password` question for a secret string input.
pub fn password<'a>(
    input: &'a StringInput<Generator>,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let validators = input.base.validators.as_slice();
    let question = Question::password(name)
        .message(input.base_extra.message.as_str())
        .validate(move |answer, _| {
            validate_for_requestty(
                &answer.to_string(),
                name,
                context_values,
                validators,
                validation_value_name,
            )
        });
    Ok(question.build())
}

/// Build a text `input` question for a plain string input.
pub fn text<'a>(
    input: &'a StringInput<Generator>,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let validators = input.base.validators.as_slice();
    let question = Question::input(name)
        .message(input.base_extra.message.as_str())
        .validate(move |answer, _| {
            validate_for_requestty(
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

/// Build a `select` question for a string input with allowed values.
pub fn select_string<'a>(
    input: &'a StringInput<Generator>,
    context_values: &'a omni_tera::Context,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let allowed = input.allowed.as_deref().unwrap_or(&[]);
    let default_value = input
        .default
        .as_deref()
        .map(|v| expand_default_value(v, name, context_values))
        .transpose()?;

    let mut question =
        Question::select(name).message(input.base_extra.message.as_str());
    for av in allowed.iter() {
        let text = string_allowed_value_text(av);
        if av.base_extra.separator {
            question = question.separator(text);
        } else {
            question = question.choice(text);
        }
    }

    Ok(if let Some(default) = default_value
        && let Some(index) = allowed
            .iter()
            .position(|o| o.value.as_str() == default.as_str())
    {
        question.default(index)
    } else {
        question
    }
    .build())
}

/// Build a `select` question for an integer input with allowed values.
pub fn select_integer<'a>(
    input: &'a IntegerInput<Generator>,
    _context_values: &'a omni_tera::Context,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let allowed = input.allowed.as_deref().unwrap_or(&[]);
    let default_index = input
        .default
        .and_then(|v| allowed.iter().position(|a| a.value == v));

    let mut question =
        Question::select(name).message(input.base_extra.message.as_str());
    for av in allowed.iter() {
        question = question.choice(integer_allowed_value_text(av));
    }

    Ok(if let Some(index) = default_index {
        question.default(index)
    } else {
        question
    }
    .build())
}

/// Build a `select` question for a float input with allowed values.
pub fn select_float<'a>(
    input: &'a FloatInput<Generator>,
    _context_values: &'a omni_tera::Context,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let allowed = input.allowed.as_deref().unwrap_or(&[]);
    let default_index = input.default.and_then(|v| {
        allowed
            .iter()
            .position(|a| (a.value - v).abs() < f64::EPSILON)
    });

    let mut question =
        Question::select(name).message(input.base_extra.message.as_str());
    for av in allowed.iter() {
        question = question.choice(float_allowed_value_text(av));
    }

    Ok(if let Some(index) = default_index {
        question.default(index)
    } else {
        question
    }
    .build())
}

/// Build a `multi_select` question for a string-array input with allowed values.
pub fn multi_select_string<'a>(
    input: &'a StringArrayInput<Generator>,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let allowed = input.body.allowed.as_deref().unwrap_or(&[]);
    let validators = input.base.validators.as_slice();

    let default_values = if let Some(defaults) = &input.body.default {
        let mut set = unordered_set!();
        for d in defaults {
            set.insert(expand_default_value(d, name, context_values)?);
        }
        Some(set)
    } else {
        None
    };

    let mut question = Question::multi_select(name)
        .message(input.base_extra.message.as_str())
        .validate(move |answers, _| {
            let values: Vec<_> = answers
                .iter()
                .enumerate()
                .filter_map(|(i, selected)| {
                    if !selected {
                        return None;
                    }
                    allowed.get(i).map(|a| a.value.clone())
                })
                .collect();
            validate_for_requestty(
                &values,
                name,
                context_values,
                validators,
                validation_value_name,
            )
        });

    if let Some(defaults) = default_values {
        for av in allowed.iter() {
            let text = string_allowed_value_text(av);
            if av.base_extra.separator {
                question = question.separator(text);
            } else {
                question = question
                    .choice_with_default(text, defaults.contains(&av.value));
            }
        }
    } else {
        for av in allowed.iter() {
            let text = string_allowed_value_text(av);
            if av.base_extra.separator {
                question = question.separator(text);
            } else {
                question = question.choice(text);
            }
        }
    }

    Ok(question.build())
}

/// Build a `multi_select` question for an integer-array input with allowed values.
pub fn multi_select_integer<'a>(
    input: &'a IntegerArrayInput<Generator>,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let allowed = input.body.allowed.as_deref().unwrap_or(&[]);
    let validators = input.base.validators.as_slice();

    let mut question = Question::multi_select(name)
        .message(input.base_extra.message.as_str())
        .validate(move |answers, _| {
            let values: Vec<_> = answers
                .iter()
                .enumerate()
                .filter_map(|(i, selected)| {
                    if !selected {
                        return None;
                    }
                    allowed.get(i).map(|a| a.value)
                })
                .collect();
            validate_for_requestty(
                &values,
                name,
                context_values,
                validators,
                validation_value_name,
            )
        });

    let defaults = input.body.default.as_deref().unwrap_or(&[]);
    for av in allowed.iter() {
        let is_default = defaults.contains(&av.value);
        if is_default {
            question = question
                .choice_with_default(integer_allowed_value_text(av), true);
        } else {
            question = question.choice(integer_allowed_value_text(av));
        }
    }

    Ok(question.build())
}

/// Build a `multi_select` question for a float-array input with allowed values.
pub fn multi_select_float<'a>(
    input: &'a FloatArrayInput<Generator>,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let allowed = input.body.allowed.as_deref().unwrap_or(&[]);
    let validators = input.base.validators.as_slice();

    let mut question = Question::multi_select(name)
        .message(input.base_extra.message.as_str())
        .validate(move |answers, _| {
            let values: Vec<_> = answers
                .iter()
                .enumerate()
                .filter_map(|(i, selected)| {
                    if !selected {
                        return None;
                    }
                    allowed.get(i).map(|a| a.value)
                })
                .collect();
            validate_for_requestty(
                &values,
                name,
                context_values,
                validators,
                validation_value_name,
            )
        });

    let defaults = input.body.default.as_deref().unwrap_or(&[]);
    for av in allowed.iter() {
        let is_default =
            defaults.iter().any(|d| (d - av.value).abs() < f64::EPSILON);
        if is_default {
            question = question
                .choice_with_default(float_allowed_value_text(av), true);
        } else {
            question = question.choice(float_allowed_value_text(av));
        }
    }

    Ok(question.build())
}

/// Build a text `input` question for numeric integer prompting.
pub fn integer_number<'a>(
    input: &'a IntegerInput<Generator>,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let validators = input.base.validators.as_slice();
    let question = Question::input(name)
        .message(input.base_extra.message.as_str())
        .validate(move |answer, _| {
            if let Ok(value) = answer.parse::<i64>() {
                validate_for_requestty(
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
    let default_value = input.default.map(|v| v.to_string());
    Ok(if let Some(default_value) = default_value {
        question.default(default_value)
    } else {
        question
    }
    .build())
}

/// Build a text `input` question for numeric float prompting.
pub fn float_number<'a>(
    input: &'a FloatInput<Generator>,
    context_values: &'a omni_tera::Context,
    validation_value_name: Option<&'a str>,
) -> Result<Question<'a>, Error> {
    let name = input.base.name.as_str();
    let validators = input.base.validators.as_slice();
    let question = Question::input(name)
        .message(input.base_extra.message.as_str())
        .validate(move |answer, _| {
            if let Ok(value) = answer.parse::<f64>() {
                validate_for_requestty(
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
    let default_value = input.default.map(|v| v.to_string());
    Ok(if let Some(default_value) = default_value {
        question.default(default_value)
    } else {
        question
    }
    .build())
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn string_allowed_value_text<'a>(
    av: &'a AllowedValue<String, Generator>,
) -> Cow<'a, str> {
    let display = av.base_extra.name.as_deref().unwrap_or(av.value.as_str());
    if av.base_extra.separator {
        return Cow::Borrowed(display);
    }
    if let Some(description) = &av.description {
        Cow::Owned(format!("{} - {} ({})", display, description, av.value))
    } else if display != av.value.as_str() {
        Cow::Owned(format!("{} ({})", display, av.value))
    } else {
        Cow::Borrowed(display)
    }
}

fn integer_allowed_value_text(av: &AllowedValue<i64, Generator>) -> String {
    let display = av.base_extra.name.as_deref().unwrap_or("");
    if !display.is_empty() {
        format!("{} ({})", display, av.value)
    } else {
        av.value.to_string()
    }
}

fn float_allowed_value_text(av: &AllowedValue<f64, Generator>) -> String {
    let display = av.base_extra.name.as_deref().unwrap_or("");
    if !display.is_empty() {
        format!("{} ({})", display, av.value)
    } else {
        av.value.to_string()
    }
}

fn expand_default_value(
    template: &str,
    name: &str,
    context_values: &omni_tera::Context,
) -> Result<String, Error> {
    omni_tera::one_off(
        template,
        &format!("default value for input {}", name),
        context_values,
    )
    .map_err(Error::from)
}

fn validate_for_requestty<T: std::fmt::Debug + Serialize + 'static>(
    value: &T,
    name: &str,
    context_values: &omni_tera::Context,
    validators: &[ValidateConfiguration],
    validation_value_name: Option<&str>,
) -> Result<(), String> {
    trace::debug!(
        validator_count = validators.len(),
        ?validation_value_name,
        ?value,
        "validate_for_requestty"
    );
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
            trace::debug!(?err, "validation_failed");
            Err(error_message.to_string())
        } else {
            trace::debug!(?err, "validation_error");
            Err(format!("unexpected validation error: {err:?}"))
        }
    } else {
        trace::debug!("validation_success");
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use omni_generator_configurations::{GenBase, Generator, StringExtras};
    use omni_input_provider::{BaseInput, BooleanInput, StringInput};

    fn make_base(name: &str) -> BaseInput {
        serde_json::from_str(&format!(r#"{{"name":"{}"}}"#, name)).unwrap()
    }

    fn make_gen_base(message: &str) -> GenBase {
        GenBase {
            message: message.to_string(),
            remember: false,
            default_expr: None,
        }
    }

    #[test]
    fn confirm_question_builds_from_boolean_input() {
        let input = BooleanInput::<Generator> {
            base: make_base("flag"),
            default: Some(true),
            base_extra: make_gen_base("Enable?"),
            profile_data: (),
        };
        let ctx = omni_tera::Context::new();
        let q = super::confirm(&input, &ctx);
        assert!(q.is_ok(), "confirm failed: {:?}", q);
    }

    #[test]
    fn text_question_builds_from_string_input() {
        let input = StringInput::<Generator> {
            base: make_base("name"),
            allowed: None,
            default: None,
            base_extra: make_gen_base("Enter name:"),
            profile_data: StringExtras::default(),
        };
        let ctx = omni_tera::Context::new();
        let q = super::text(&input, &ctx, Some("value"));
        assert!(q.is_ok(), "text failed: {:?}", q);
    }

    #[test]
    fn password_question_builds_from_string_input() {
        let input = StringInput::<Generator> {
            base: make_base("token"),
            allowed: None,
            default: None,
            base_extra: make_gen_base("Enter token:"),
            profile_data: StringExtras::default(),
        };
        let ctx = omni_tera::Context::new();
        let q = super::password(&input, &ctx, Some("value"));
        assert!(q.is_ok(), "password failed: {:?}", q);
    }

    #[test]
    fn integer_number_question_builds() {
        use omni_input_provider::IntegerInput;
        let input = IntegerInput::<Generator> {
            base: make_base("count"),
            allowed: None,
            default: Some(3),
            base_extra: make_gen_base("How many?"),
            profile_data: (),
        };
        let ctx = omni_tera::Context::new();
        let q = super::integer_number(&input, &ctx, Some("value"));
        assert!(q.is_ok(), "integer_number failed: {:?}", q);
    }

    #[test]
    fn float_number_question_builds() {
        use omni_input_provider::FloatInput;
        let input = FloatInput::<Generator> {
            base: make_base("rate"),
            allowed: None,
            default: Some(1.5),
            base_extra: make_gen_base("What rate?"),
            profile_data: (),
        };
        let ctx = omni_tera::Context::new();
        let q = super::float_number(&input, &ctx, Some("value"));
        assert!(q.is_ok(), "float_number failed: {:?}", q);
    }
}
