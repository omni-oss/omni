use std::marker::PhantomData;

use serde::Deserialize;

use crate::StaticValidator;

pub struct Validated<T: for<'de> Deserialize<'de>, V: StaticValidator<T>>(
    T,
    PhantomData<V>,
);

impl<'de, T: for<'de2> Deserialize<'de2>, V: StaticValidator<T>>
    Deserialize<'de> for Validated<T, V>
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let t = T::deserialize(deserializer)?;

        V::validate_static(&t).map_err(serde::de::Error::custom)?;

        Ok(Validated(t, PhantomData))
    }
}
