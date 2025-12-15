use serde_repr::{Deserialize_repr, Serialize_repr};

#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    Deserialize_repr,
    Serialize_repr,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    strum::FromRepr,
    strum::Display,
    strum::EnumIs,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResponseErrorCode {
    NotFound,
    InvalidRequest,
    InternalError,
}

#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    Deserialize_repr,
    Serialize_repr,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    strum::FromRepr,
    strum::Display,
    strum::EnumIs,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestErrorCode {
    Cancelled,
    Timeout,
}
