pub mod default_hash_to_string {
    use base64::Engine;
    use omni_hasher::impls::DefaultHash;
    use serde::Deserialize as _;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DefaultHash, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let b = base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|e| {
                serde::de::Error::custom(format!(
                    "failed to decode base64 string: {e}"
                ))
            })?;

        if b.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "expected a 32 byte hash, got {b:?}"
            )));
        }

        let mut h = [0u8; 32];

        h.copy_from_slice(&b);

        Ok(h)
    }

    pub fn serialize<S>(
        h: &DefaultHash,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let b = base64::engine::general_purpose::STANDARD.encode(h);

        serializer.serialize_str(&b)
    }
}

pub mod default_hash_option_to_string {
    use omni_hasher::impls::DefaultHash;
    use serde::{Deserialize as _, de::IntoDeserializer as _};

    #[allow(unused)]
    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<DefaultHash>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = Option::<String>::deserialize(deserializer)?;
        if let Some(s) = s {
            super::default_hash_to_string::deserialize(s.into_deserializer())
                .map(Some)
        } else {
            Ok(None)
        }
    }

    #[allow(unused)]
    pub fn serialize<S>(
        h: &Option<DefaultHash>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some(h) = h {
            super::default_hash_to_string::serialize(h, serializer)
        } else {
            serializer.serialize_none()
        }
    }
}
