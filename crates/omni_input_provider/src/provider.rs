use async_trait::async_trait;
use omni_input_schema::{
    BooleanInput, FloatArrayInput, FloatInput, InputProfile, IntegerArrayInput,
    IntegerInput, StringArrayInput, StringInput,
};

use crate::error::Error;

/// Each method corresponds to one `Input<E>` variant.
///
/// The provider receives the **full** variant struct so it can inspect
/// both data signals (e.g. `allowed`, `secret`) and presentation extras
/// (e.g. `base_extra.message`, `profile_data.widget`) when deciding how
/// to collect the value.
///
/// Widget / presentation inference — picking password vs text, select vs
/// free-entry, etc. — lives **here**, not in `collect()`.  `collect()` only
/// dispatches to the right method based on the variant.
///
/// Implementations are **not** responsible for validation; that is handled
/// by `collect()` using `omni_input_schema::validate`.
#[async_trait]
pub trait InputProvider<E: InputProfile + Send + Sync + 'static>:
    Send + Sync + std::fmt::Debug
{
    async fn boolean(
        &self,
        input: &BooleanInput<E>,
        ctx: &omni_tera::Context,
    ) -> Result<bool, Error>;

    async fn string(
        &self,
        input: &StringInput<E>,
        ctx: &omni_tera::Context,
    ) -> Result<String, Error>;

    async fn integer(
        &self,
        input: &IntegerInput<E>,
        ctx: &omni_tera::Context,
    ) -> Result<i64, Error>;

    async fn float(
        &self,
        input: &FloatInput<E>,
        ctx: &omni_tera::Context,
    ) -> Result<f64, Error>;

    async fn string_array(
        &self,
        input: &StringArrayInput<E>,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<String>, Error>;

    async fn integer_array(
        &self,
        input: &IntegerArrayInput<E>,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<i64>, Error>;

    async fn float_array(
        &self,
        input: &FloatArrayInput<E>,
        ctx: &omni_tera::Context,
    ) -> Result<Vec<f64>, Error>;
}
