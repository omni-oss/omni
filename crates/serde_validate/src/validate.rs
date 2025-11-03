use serde::Deserialize;

use crate::{StaticValidator, Validator};

pub fn validate<
    'de,
    V: Validator<T> + Default,
    T: Deserialize<'de>,
    D: serde::de::Deserializer<'de>,
>(
    deserializer: D,
) -> Result<T, D::Error> {
    let serialized = T::deserialize(deserializer)?;
    let validator = V::default();
    let result = validator.validate(&serialized);
    if let Err(error) = result {
        return Err(serde::de::Error::custom(error));
    }

    Ok(serialized)
}

pub fn option_validate<
    'de,
    V: Validator<T> + Default,
    T: Deserialize<'de>,
    D: serde::de::Deserializer<'de>,
>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let serialized = Option::<T>::deserialize(deserializer)?;

    if let Some(serialized) = &serialized {
        let validator = V::default();
        let result = validator.validate(serialized);
        if let Err(error) = result {
            return Err(serde::de::Error::custom(error));
        }
    }

    Ok(serialized)
}

pub fn validate_static<
    'de,
    V: StaticValidator<T>,
    T: Deserialize<'de>,
    D: serde::de::Deserializer<'de>,
>(
    deserializer: D,
) -> Result<T, D::Error> {
    let serialized = T::deserialize(deserializer)?;
    let result = V::validate_static(&serialized);
    if let Err(error) = result {
        return Err(serde::de::Error::custom(error));
    }

    Ok(serialized)
}

pub fn option_validate_static<
    'de,
    V: StaticValidator<T>,
    T: Deserialize<'de>,
    D: serde::de::Deserializer<'de>,
>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let serialized = Option::<T>::deserialize(deserializer)?;

    if let Some(serialized) = &serialized {
        let result = V::validate_static(serialized);
        if let Err(error) = result {
            return Err(serde::de::Error::custom(error));
        }
    }

    Ok(serialized)
}

pub macro declare_validator($validator_type:ty, $type:ty, $name:ident, $option_name:ident $(,)?) {
    pub fn $name<'de, D: serde::de::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<$type, D::Error> {
        $crate::validate::<$validator_type, $type, _>(deserializer)
    }

    pub fn $option_name<'de, D: serde::de::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<$type>, D::Error> {
        $crate::option_validate::<$validator_type, $type, _>(deserializer)
    }
}

pub macro declare_static_validator($validator_type:ty, $type:ty, $name:ident, $option_name:ident $(,)?) {
    pub fn $name<'de, D: serde::de::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<$type, D::Error> {
        $crate::validate_static::<$validator_type, $type, _>(deserializer)
    }

    pub fn $option_name<'de, D: serde::de::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<$type>, D::Error> {
        $crate::option_validate_static::<$validator_type, $type, _>(
            deserializer,
        )
    }
}
