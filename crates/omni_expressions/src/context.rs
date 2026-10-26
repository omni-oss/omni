use cel::{common::value::CowVal, objects::TryIntoValue};

use crate::ErrorInner;

#[derive(Default)]
pub struct Context<'p, 'v> {
    pub(crate) inner: cel::Context<'p, 'v>,
}

impl<'p, 'v> Context<'p, 'v> {
    pub fn add_variable<S, V>(
        &mut self,
        name: S,
        value: V,
    ) -> Result<(), crate::Error>
    where
        S: Into<String>,
        V: TryIntoValue,
    {
        Ok(self
            .inner
            .add_variable(name, value)
            .map_err(|e| eyre::eyre!(e))?)
    }

    pub fn get_variable<S>(
        &self,
        name: S,
    ) -> Result<CowVal<'_, '_>, crate::Error>
    where
        S: AsRef<str>,
    {
        let value = self
            .inner
            .get_variable(name.as_ref())
            .ok_or_else(|| ErrorInner::new_variable_not_found(name.as_ref()))?;
        Ok(value)
    }
}
