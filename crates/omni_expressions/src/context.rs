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

impl<'p, 'v> Context<'p, 'v> {
    /// Registers `root` as a map variable holding all `entries`, and each entry
    /// as a top-level variable too. Entries are converted once; the clones that
    /// back the extra registrations share `Arc` storage. A top-level entry named
    /// `root` wins and `root` is then not injected.
    pub fn add_map_with_root<S, V, I>(
        &mut self,
        root: &str,
        entries: I,
    ) -> Result<(), crate::Error>
    where
        S: Into<String>,
        V: TryIntoValue,
        I: IntoIterator<Item = (S, V)>,
    {
        use cel::objects::{Key, Map};
        use std::collections::HashMap;

        let mut map: HashMap<Key, cel::Value> = HashMap::new();
        let mut has_root = false;
        for (k, v) in entries {
            let k: String = k.into();
            let value =
                v.try_into_value().map_err(|e| eyre::eyre!(e))?;
            if k == root {
                has_root = true;
            }
            self.inner.add_variable_from_value(k.clone(), value.clone());
            map.insert(Key::from(k), value);
        }

        if !has_root {
            self.inner.add_variable_from_value(
                root,
                cel::Value::Map(Map {
                    map: std::sync::Arc::new(map),
                }),
            );
        }
        Ok(())
    }
}
