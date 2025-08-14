use cel::{Value, objects::TryIntoValue};

#[derive(Default)]
pub struct Context<'a> {
    pub(crate) inner: cel::Context<'a>,
}

impl<'a> Context<'a> {
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

    pub fn get_variable<S>(&self, name: S) -> Result<Value, crate::Error>
    where
        S: AsRef<str>,
    {
        Ok(self.inner.get_variable(name)?)
    }
}
