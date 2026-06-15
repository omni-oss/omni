use async_trait::async_trait;

use crate::{
    configuration::{
        ConfirmInputConfiguration, FloatInputConfiguration,
        IntegerInputConfiguration, MultiSelectInputConfiguration,
        PasswordInputConfiguration, SelectInputConfiguration,
        TextInputConfiguration,
    },
    error::Error,
};

/// Implementations are **not** responsible for validation — that is handled
/// centrally in [`collect`][crate::collect] so that the re-ask-on-invalid
/// loop works identically across all surfaces (CLI, MCP, UI, …).
///
/// The [`omni_tera::Context`] is provided so implementations can expand
/// template expressions in default values or dynamic messages if they choose.
#[async_trait]
pub trait InputProvider: Send + Sync + std::fmt::Debug {
    async fn confirm(
        &self,
        input: &ConfirmInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<bool, Error>;

    async fn text(
        &self,
        input: &TextInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error>;

    async fn password(
        &self,
        input: &PasswordInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error>;

    /// Returns the selected option's `value` field.
    async fn select(
        &self,
        input: &SelectInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error>;

    /// Returns the selected options' `value` fields.
    async fn multi_select(
        &self,
        input: &MultiSelectInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, Error>;

    async fn float_number(
        &self,
        input: &FloatInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<f64, Error>;

    async fn integer_number(
        &self,
        input: &IntegerInputConfiguration,
        ctx: &omni_tera::Context,
    ) -> Result<i64, Error>;
}
