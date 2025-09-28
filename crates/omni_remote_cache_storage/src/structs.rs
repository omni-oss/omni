use bytesize::ByteSize;
use derive_new::new;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    new,
    ToSchema,
)]
pub struct ListItem {
    pub key: String,
    #[schema(value_type = u64)]
    #[serde(with = "bytesize_serde")]
    pub size: ByteSize,
}

mod bytesize_serde {
    use bytesize::ByteSize;
    use serde::Deserialize;

    pub fn serialize<S>(
        size: &ByteSize,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(size.as_u64())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<ByteSize, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes = u64::deserialize(deserializer)?;

        Ok(ByteSize::b(bytes))
    }
}

impl ListItem {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn size(&self) -> ByteSize {
        self.size
    }
}
