use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::predefined_codes;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
)]
#[serde(transparent)]
pub struct ResponseErrorCode(u16);

impl Display for ResponseErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl ResponseErrorCode {
    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    pub const fn code(&self) -> u16 {
        self.0
    }
}

predefined_codes!(ResponseErrorCode {
    UNEXPECTED_FRAME = 0;
});

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
)]
#[serde(transparent)]
pub struct RequestErrorCode(u16);

impl Display for RequestErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl RequestErrorCode {
    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    pub const fn code(&self) -> u16 {
        self.0
    }
}

predefined_codes!(RequestErrorCode {
    UNEXPECTED_FRAME = 0;
    TIMED_OUT = 1;
});
