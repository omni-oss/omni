use async_trait::async_trait;
use omni_input_provider::{
    configuration::{
        ConfirmInputConfiguration, FloatInputConfiguration,
        IntegerInputConfiguration, MultiSelectInputConfiguration,
        PasswordInputConfiguration, SelectInputConfiguration,
        TextInputConfiguration,
    },
    error::Error,
    provider::InputProvider,
};

/// `requestty::prompt_one` is synchronous blocking; each `async` method calls
/// it directly. The `async` surface exists for trait compatibility with
/// non-blocking backends (MCP, UI, …), not because the CLI needs async I/O.
#[derive(Debug)]
pub struct CliInputProvider {
    /// Variable name used in inline validator Tera expressions.
    /// Defaults to `Some("value")`, matching the generator YAML convention.
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
impl InputProvider for CliInputProvider {
    async fn confirm(
        &self,
        input: &ConfirmInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<bool, Error> {
        let question = crate::make::confirm(input, ctx)?;
        let answer = requestty::prompt_one(question)
            .map_err(|e| Error::from(eyre::eyre!("input error: {e}")))?;
        answer
            .as_bool()
            .ok_or_else(|| Error::from(eyre::eyre!("expected boolean answer")))
    }

    async fn text(
        &self,
        input: &TextInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        let question =
            crate::make::text(input, ctx, self.validation_value_name)?;
        let answer = requestty::prompt_one(question)
            .map_err(|e| Error::from(eyre::eyre!("input error: {e}")))?;
        answer
            .as_string()
            .map(str::to_string)
            .ok_or_else(|| Error::from(eyre::eyre!("expected string answer")))
    }

    async fn password(
        &self,
        input: &PasswordInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        let question =
            crate::make::password(input, ctx, self.validation_value_name)?;
        let answer = requestty::prompt_one(question)
            .map_err(|e| Error::from(eyre::eyre!("input error: {e}")))?;
        answer
            .as_string()
            .map(str::to_string)
            .ok_or_else(|| Error::from(eyre::eyre!("expected string answer")))
    }

    async fn select(
        &self,
        input: &SelectInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        let question = crate::make::select(input, ctx)?;
        let answer = requestty::prompt_one(question)
            .map_err(|e| Error::from(eyre::eyre!("input error: {e}")))?;
        answer
            .as_list_item()
            .map(|i| input.options[i.index].value.clone())
            .ok_or_else(|| {
                Error::from(eyre::eyre!("expected list-item answer"))
            })
    }

    async fn multi_select(
        &self,
        input: &MultiSelectInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, Error> {
        let question =
            crate::make::multi_select(input, ctx, self.validation_value_name)?;
        let answer = requestty::prompt_one(question)
            .map_err(|e| Error::from(eyre::eyre!("input error: {e}")))?;
        let values = answer
            .as_list_items()
            .ok_or_else(|| {
                Error::from(eyre::eyre!("expected list-items answer"))
            })?
            .iter()
            .map(|i| input.options[i.index].value.clone())
            .collect();
        Ok(values)
    }

    async fn float_number(
        &self,
        input: &FloatInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<f64, Error> {
        let question =
            crate::make::float_number(input, ctx, self.validation_value_name)?;
        let answer = requestty::prompt_one(question)
            .map_err(|e| Error::from(eyre::eyre!("input error: {e}")))?;
        // float_number uses a text input with parse-time validation
        let raw = answer.as_string().ok_or_else(|| {
            Error::from(eyre::eyre!("expected string answer"))
        })?;
        raw.parse::<f64>()
            .map_err(|e| Error::from(eyre::eyre!("cannot parse float: {e}")))
    }

    async fn integer_number(
        &self,
        input: &IntegerInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<i64, Error> {
        let question = crate::make::integer_number(
            input,
            ctx,
            self.validation_value_name,
        )?;
        let answer = requestty::prompt_one(question)
            .map_err(|e| Error::from(eyre::eyre!("input error: {e}")))?;
        let raw = answer.as_string().ok_or_else(|| {
            Error::from(eyre::eyre!("expected string answer"))
        })?;
        raw.parse::<i64>()
            .map_err(|e| Error::from(eyre::eyre!("cannot parse integer: {e}")))
    }
}
