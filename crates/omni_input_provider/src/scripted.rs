use std::collections::HashMap;

use async_trait::async_trait;
use omni_input_schema::{
    BooleanInput, FloatArrayInput, FloatInput, InputProfile, IntegerArrayInput,
    IntegerInput, StringArrayInput, StringInput,
};

use crate::{error::Error, provider::InputProvider};

/// Test harness provider. Answers are supplied as raw strings keyed by input
/// name and parsed to the required type at call time, mirroring how a real
/// user types responses.  A missing key causes an immediate error.
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
                "ScriptedInputProvider: no answer for '{name}'"
            ))
        })
    }
}

#[async_trait]
impl<E: InputProfile + Send + Sync + 'static> InputProvider<E>
    for ScriptedInputProvider
{
    async fn boolean(
        &self,
        input: &BooleanInput<E>,
        _ctx: &omni_tera::Context,
    ) -> Result<bool, Error> {
        let raw = self.get(&input.base.name)?;
        raw.parse::<bool>().map_err(|e| {
            Error::from(eyre::eyre!(
                "ScriptedInputProvider: cannot parse '{raw}' as bool \
                 for input '{}': {e}",
                input.base.name
            ))
        })
    }

    async fn string(
        &self,
        input: &StringInput<E>,
        _ctx: &omni_tera::Context,
    ) -> Result<String, Error> {
        Ok(self.get(&input.base.name)?.to_string())
    }

    async fn integer(
        &self,
        input: &IntegerInput<E>,
        _ctx: &omni_tera::Context,
    ) -> Result<i64, Error> {
        let raw = self.get(&input.base.name)?;
        raw.parse::<i64>().map_err(|e| {
            Error::from(eyre::eyre!(
                "ScriptedInputProvider: cannot parse '{raw}' as i64 \
                 for input '{}': {e}",
                input.base.name
            ))
        })
    }

    async fn float(
        &self,
        input: &FloatInput<E>,
        _ctx: &omni_tera::Context,
    ) -> Result<f64, Error> {
        let raw = self.get(&input.base.name)?;
        raw.parse::<f64>().map_err(|e| {
            Error::from(eyre::eyre!(
                "ScriptedInputProvider: cannot parse '{raw}' as f64 \
                 for input '{}': {e}",
                input.base.name
            ))
        })
    }

    async fn string_array(
        &self,
        input: &StringArrayInput<E>,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, Error> {
        let raw = self.get(&input.base.name)?;
        Ok(raw.split(',').map(|s| s.trim().to_string()).collect())
    }

    async fn integer_array(
        &self,
        input: &IntegerArrayInput<E>,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<i64>, Error> {
        let raw = self.get(&input.base.name)?;
        raw.split(',')
            .map(|s| {
                let s = s.trim();
                s.parse::<i64>().map_err(|e| {
                    Error::from(eyre::eyre!(
                        "ScriptedInputProvider: cannot parse '{s}' as i64 \
                         for input '{}': {e}",
                        input.base.name
                    ))
                })
            })
            .collect()
    }

    async fn float_array(
        &self,
        input: &FloatArrayInput<E>,
        _ctx: &omni_tera::Context,
    ) -> Result<Vec<f64>, Error> {
        let raw = self.get(&input.base.name)?;
        raw.split(',')
            .map(|s| {
                let s = s.trim();
                s.parse::<f64>().map_err(|e| {
                    Error::from(eyre::eyre!(
                        "ScriptedInputProvider: cannot parse '{s}' as f64 \
                         for input '{}': {e}",
                        input.base.name
                    ))
                })
            })
            .collect()
    }
}
