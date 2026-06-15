#[allow(unused)]
pub mod either_value_or_tera_expr {
    use either::{
        self,
        Either::{self, Right},
        serde_untagged,
    };
    use omni_serde_validators::tera_expr::TeraExprValidator;
    use serde::{Serialize, Serializer};
    use serde_validate::StaticValidator;

    pub fn deserialize<
        'de,
        L: serde::Deserialize<'de>,
        D: serde::de::Deserializer<'de>,
    >(
        deserializer: D,
    ) -> Result<Either<L, String>, D::Error> {
        let value: Either<L, String> =
            serde_untagged::deserialize(deserializer)?;

        if let Either::Right(value) = value {
            let result = TeraExprValidator::validate_static(&value);

            return match result {
                Ok(_) => Ok(Right(value)),
                Err(e) => Err(serde::de::Error::custom(e)),
            };
        }

        Ok(value)
    }

    pub fn serialize<L, S>(
        this: &Either<L, String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        L: Serialize,
    {
        return serde_untagged::serialize(this, serializer);
    }
}

pub mod either_value_or_tera_expr_optional {
    use either::{
        Either::{self, Right},
        serde_untagged_optional,
    };
    use omni_serde_validators::tera_expr::TeraExprValidator;
    use serde_validate::StaticValidator as _;

    pub fn deserialize<
        'de,
        L: serde::Deserialize<'de>,
        D: serde::de::Deserializer<'de>,
    >(
        deserializer: D,
    ) -> Result<Option<Either<L, String>>, D::Error> {
        let value: Option<Either<L, String>> =
            serde_untagged_optional::deserialize(deserializer)?;

        if let Some(Either::Right(value)) = value {
            let result = TeraExprValidator::validate_static(&value);

            return match result {
                Ok(_) => Ok(Some(Right(value))),
                Err(e) => Err(serde::de::Error::custom(e)),
            };
        } else {
        }

        Ok(value)
    }

    pub fn serialize<L, S>(
        this: &Option<either::Either<L, String>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
        L: serde::Serialize,
    {
        return serde_untagged_optional::serialize(this, serializer);
    }
}
