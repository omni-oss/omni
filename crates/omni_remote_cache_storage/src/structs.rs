use bytesize::ByteSize;
use derive_new::new;
use serde::{Deserialize, Serialize};

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
)]
pub struct ListItem {
    pub key: String,
    pub size: ByteSize,
}

impl ListItem {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn size(&self) -> ByteSize {
        self.size
    }
}
