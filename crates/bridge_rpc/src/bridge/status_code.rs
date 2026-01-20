use serde::{Deserialize, Serialize};

use crate::predefined_codes;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize,
)]
#[serde(transparent)]
pub struct ResponseStatusCode(u16);

impl ResponseStatusCode {
    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    pub const fn code(&self) -> u16 {
        self.0
    }
}

predefined_codes!(ResponseStatusCode {
    SUCCESS = 0;
    NO_HANDLER_FOR_PATH = 100;
});
