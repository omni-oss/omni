use std::collections::HashMap;

use async_trait::async_trait;

use crate::{
    configuration::{
        ConfirmInputConfiguration, FloatInputConfiguration,
        IntegerInputConfiguration, MultiSelectInputConfiguration,
        PasswordInputConfiguration, SelectInputConfiguration,
        TextInputConfiguration,
    },
    error::Error,
    provider::InputProvider,
};

/// Intended for unit tests and integration harnesses.
///
/// Answers are given as raw strings and parsed to the required type, which
/// mirrors how a CLI user types their responses. A missing key returns an
/// error so tests fail fast rather than blocking on stdin.
#[derive(Debug)]
pub struct ScriptedInputProvider {
    answers: HashMap<String, String>,
}

impl ScriptedInputProvider {
    pub fn new(
        answers: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        Self {
            answers: answers
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    fn get(&self, name: &str) -> Result<&str, Error> {
        self.answers.get(name).map(|s| s.as_str()).ok_or_else(|| {
            Error::from(eyre::eyre!(
                "ScriptedInputProvider: no answer configured for input '{name}'"
            ))
        })
    }
}

#[async_trait]
impl InputProvider for ScriptedInputProvider {
    async fn confirm(
        &self,
        input: &ConfirmInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<bool, Error> {
        let raw = self.get(&input.base.name)?;
        raw.parse::<bool>().map_err(|e| {
            Error::from(eyre::eyre!(
                "ScriptedInputProvider: cannot parse '{}' as bool for input '{}': {e}",
                raw,
                input.base.name
            ))
        })
    }

    async fn text(
        &self,
        input: &TextInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        Ok(self.get(&input.base.base.name)?.to_string())
    }

    async fn password(
        &self,
        input: &PasswordInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        Ok(self.get(&input.base.base.name)?.to_string())
    }

    async fn select(
        &self,
        input: &SelectInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        let raw = self.get(&input.base.name)?;
        // Accept either the option value directly or the option name.
        let matched = input
            .options
            .iter()
            .find(|o| o.value == raw || o.name == raw)
            .map(|o| o.value.clone())
            .unwrap_or_else(|| raw.to_string());
        Ok(matched)
    }

    async fn multi_select(
        &self,
        input: &MultiSelectInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, Error> {
        let raw = self.get(&input.base.base.name)?;
        // Comma-separated list of values/names.
        let selected: Vec<String> = raw
            .split(',')
            .map(|s| s.trim())
            .map(|s| {
                input
                    .options
                    .iter()
                    .find(|o| o.value == s || o.name == s)
                    .map(|o| o.value.clone())
                    .unwrap_or_else(|| s.to_string())
            })
            .collect();
        Ok(selected)
    }

    async fn float_number(
        &self,
        input: &FloatInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<f64, Error> {
        let raw = self.get(&input.base.base.name)?;
        raw.parse::<f64>().map_err(|e| {
            Error::from(eyre::eyre!(
                "ScriptedInputProvider: cannot parse '{}' as f64 for input '{}': {e}",
                raw,
                input.base.base.name
            ))
        })
    }

    async fn integer_number(
        &self,
        input: &IntegerInputConfiguration,
        _ctx: &omni_tera::Context,
    ) -> Result<i64, Error> {
        let raw = self.get(&input.base.base.name)?;
        raw.parse::<i64>().map_err(|e| {
            Error::from(eyre::eyre!(
                "ScriptedInputProvider: cannot parse '{}' as i64 for input '{}': {e}",
                raw,
                input.base.base.name
            ))
        })
    }
}
