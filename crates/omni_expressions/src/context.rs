use cel::{ExecutionError, Value, objects::TryIntoValue};

#[derive(Default)]
pub struct Context<'a> {
    pub(crate) inner: cel::Context<'a>,
}

impl<'a> Context<'a> {
    pub fn add_variable<S, V>(
        &mut self,
        name: S,
        value: V,
    ) -> Result<(), <V as TryIntoValue>::Error>
    where
        S: Into<String>,
        V: TryIntoValue,
    {
        self.inner.add_variable(name, value)
    }

    pub fn get_variable<S>(&self, name: S) -> Result<Value, ExecutionError>
    where
        S: AsRef<str>,
    {
        self.inner.get_variable(name)
    }
}
