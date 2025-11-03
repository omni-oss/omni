use crate::configuration::{
    CheckboxPromptConfiguration, FloatNumberPromptConfiguration,
    IntegerNumberPromptConfiguration, MultiSelectPromptConfiguration,
    PasswordPromptConfiguration, PromptConfiguration,
    SelectPromptConfiguration, TextPromptConfiguration,
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
    validate_prompts(prompts)?;

    let mut values = UnorderedMap::default();

    for prompt in prompts {
        let (key, value) = match prompt {
            PromptConfiguration::Checkbox { prompt } => {
                let value = prompt_checkbox(prompt)?;

                (prompt.base.name.clone(), value)
            }
            PromptConfiguration::Select { prompt } => {
                let value = prompt_select(prompt)?;

                (prompt.base.name.clone(), value)
            }
            PromptConfiguration::MultiSelect { prompt } => {
                let value = prompt_multi_select(prompt)?;

                (prompt.base.name.clone(), value)
            }
            PromptConfiguration::Text { prompt } => {
                let value = prompt_text(prompt)?;

                (prompt.base.base.name.clone(), value)
            }
            PromptConfiguration::Password { prompt } => {
                let value = prompt_password(prompt)?;

                (prompt.base.base.name.clone(), value)
            }
            PromptConfiguration::FloatNumber { prompt } => {
                let value = prompt_float_number(prompt)?;

                (prompt.base.base.name.clone(), value)
            }
            PromptConfiguration::IntegerNumber { prompt } => {
                let value = prompt_integer_number(prompt)?;

                (prompt.base.base.name.clone(), value)
            }
        };

        values.insert(key, value);
    }

    Ok(values)
}

fn validate_prompts(
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
}
