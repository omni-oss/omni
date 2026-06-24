use async_trait::async_trait;
use omni_generator_configurations::{Generator, ListWidget, StringWidget};
use omni_input_provider::{
    BooleanInput, FloatArrayInput, FloatInput, InputProvider,
    IntegerArrayInput, IntegerInput, StringArrayInput, StringInput,
    error::Error,
};
use requestty::Question;

use crate::make;

#[derive(Debug)]
pub struct CliInputProvider {
    validation_value_name: Option<&'static str>,
}

impl Default for CliInputProvider {
    fn default() -> Self {
        Self {
            validation_value_name: Some("value"),
        }
    }
}

#[async_trait]
impl InputProvider<Generator> for CliInputProvider {
    async fn boolean(
        &self,
        input: &BooleanInput<Generator>,
        ctx: &omni_tera::Context,
    ) -> Result<bool, Error> {
        let question = make::confirm(input, ctx)?;
        let answer =
            tokio::task::block_in_place(|| requestty::prompt_one(question))
                .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
        answer
            .try_into_bool()
            .map_err(|_| eyre::eyre!("expected bool answer").into())
    }

    async fn string(
        &self,
        input: &StringInput<Generator>,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        let question = if input.profile_data.widget
            == Some(StringWidget::Password)
            || input.base.secret
        {
            make::password(input, ctx, self.validation_value_name)?
        } else if input.profile_data.widget == Some(StringWidget::Select)
            || input.allowed.is_some()
        {
            make::select_string(input, ctx)?
        } else {
            make::text(input, ctx, self.validation_value_name)?
        };

        let answer =
            tokio::task::block_in_place(|| requestty::prompt_one(question))
                .map_err(|e| eyre::eyre!("prompt error: {e}"))?;

        if input.profile_data.widget == Some(StringWidget::Select)
            || input.allowed.is_some()
        {
            // select returns a ListItem; look up the actual value by index
            let idx = answer
                .as_list_item()
                .ok_or_else(|| eyre::eyre!("expected list item answer"))?
                .index;
            let allowed = input.allowed.as_ref().ok_or_else(|| {
                eyre::eyre!(
                    "select question produced ListItem but allowed is empty"
                )
            })?;
            Ok(allowed[idx].value.clone())
        } else {
            answer
                .try_into_string()
                .map_err(|_| eyre::eyre!("expected string answer").into())
        }
    }

    async fn integer(
        &self,
        input: &IntegerInput<Generator>,
        ctx: &omni_tera::Context,
    ) -> Result<i64, Error> {
        if input.allowed.is_some() {
            let question = make::select_integer(input, ctx)?;
            let answer =
                tokio::task::block_in_place(|| requestty::prompt_one(question))
                    .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            let idx = answer
                .as_list_item()
                .ok_or_else(|| eyre::eyre!("expected list item answer"))?
                .index;
            let allowed = input.allowed.as_ref().unwrap();
            Ok(allowed[idx].value)
        } else {
            let question =
                make::integer_number(input, ctx, self.validation_value_name)?;
            let answer =
                tokio::task::block_in_place(|| requestty::prompt_one(question))
                    .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            let s = answer
                .try_into_string()
                .map_err(|_| eyre::eyre!("expected string answer"))?;
            s.parse::<i64>()
                .map_err(|e| eyre::eyre!("parse int: {e}").into())
        }
    }

    async fn float(
        &self,
        input: &FloatInput<Generator>,
        ctx: &omni_tera::Context,
    ) -> Result<f64, Error> {
        if input.allowed.is_some() {
            let question = make::select_float(input, ctx)?;
            let answer =
                tokio::task::block_in_place(|| requestty::prompt_one(question))
                    .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            let idx = answer
                .as_list_item()
                .ok_or_else(|| eyre::eyre!("expected list item answer"))?
                .index;
            let allowed = input.allowed.as_ref().unwrap();
            Ok(allowed[idx].value)
        } else {
            let question =
                make::float_number(input, ctx, self.validation_value_name)?;
            let answer =
                tokio::task::block_in_place(|| requestty::prompt_one(question))
                    .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            let s = answer
                .try_into_string()
                .map_err(|_| eyre::eyre!("expected string answer"))?;
            s.parse::<f64>()
                .map_err(|e| eyre::eyre!("parse float: {e}").into())
        }
    }

    async fn string_array(
        &self,
        input: &StringArrayInput<Generator>,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, Error> {
        if input.profile_data.widget == Some(ListWidget::FreeEntry)
            || input.body.allowed.is_none()
        {
            let name = input.base.name.as_str();
            let message = input.base_extra.message.as_str();
            tokio::task::block_in_place(|| {
                prompt_free_entry_list(name, message)
            })
            .map_err(|e| eyre::eyre!("prompt error: {e}").into())
        } else {
            let question = make::multi_select_string(
                input,
                ctx,
                self.validation_value_name,
            )?;
            let answer =
                tokio::task::block_in_place(|| requestty::prompt_one(question))
                    .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            let items = answer
                .try_into_list_items()
                .map_err(|_| eyre::eyre!("expected list items answer"))?;
            let allowed = input.body.allowed.as_ref().unwrap();
            Ok(items
                .iter()
                .map(|item| allowed[item.index].value.clone())
                .collect())
        }
    }

    async fn integer_array(
        &self,
        input: &IntegerArrayInput<Generator>,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<i64>, Error> {
        if input.profile_data.widget == Some(ListWidget::FreeEntry)
            || input.body.allowed.is_none()
        {
            let name = input.base.name.as_str();
            let message = input.base_extra.message.as_str();
            let strings = tokio::task::block_in_place(|| {
                prompt_free_entry_list(name, message)
            })
            .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            strings
                .into_iter()
                .map(|s| {
                    s.trim()
                        .parse::<i64>()
                        .map_err(|e| eyre::eyre!("parse int: {e}").into())
                })
                .collect()
        } else {
            let question = make::multi_select_integer(
                input,
                ctx,
                self.validation_value_name,
            )?;
            let answer =
                tokio::task::block_in_place(|| requestty::prompt_one(question))
                    .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            let items = answer
                .try_into_list_items()
                .map_err(|_| eyre::eyre!("expected list items answer"))?;
            let allowed = input.body.allowed.as_ref().unwrap();
            Ok(items.iter().map(|item| allowed[item.index].value).collect())
        }
    }

    async fn float_array(
        &self,
        input: &FloatArrayInput<Generator>,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<f64>, Error> {
        if input.profile_data.widget == Some(ListWidget::FreeEntry)
            || input.body.allowed.is_none()
        {
            let name = input.base.name.as_str();
            let message = input.base_extra.message.as_str();
            let strings = tokio::task::block_in_place(|| {
                prompt_free_entry_list(name, message)
            })
            .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            strings
                .into_iter()
                .map(|s| {
                    s.trim()
                        .parse::<f64>()
                        .map_err(|e| eyre::eyre!("parse float: {e}").into())
                })
                .collect()
        } else {
            let question = make::multi_select_float(
                input,
                ctx,
                self.validation_value_name,
            )?;
            let answer =
                tokio::task::block_in_place(|| requestty::prompt_one(question))
                    .map_err(|e| eyre::eyre!("prompt error: {e}"))?;
            let items = answer
                .try_into_list_items()
                .map_err(|_| eyre::eyre!("expected list items answer"))?;
            let allowed = input.body.allowed.as_ref().unwrap();
            Ok(items.iter().map(|item| allowed[item.index].value).collect())
        }
    }
}

/// Prompt the user for items one at a time until they submit an empty line.
fn prompt_free_entry_list(
    name: &str,
    message: &str,
) -> requestty::Result<Vec<String>> {
    let mut results = Vec::new();
    loop {
        let prompt_msg = if results.is_empty() {
            format!("{} (enter items one at a time, empty to finish)", message)
        } else {
            format!("{} (item {}, empty to finish)", message, results.len() + 1)
        };
        let question = Question::input(name).message(prompt_msg).build();
        let answer = requestty::prompt_one(question)?;
        let text = answer.try_into_string().unwrap_or_default();
        if text.trim().is_empty() {
            break;
        }
        results.push(text);
    }
    Ok(results)
}
