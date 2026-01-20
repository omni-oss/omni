use serde_repr::{Deserialize_repr, Serialize_repr};

#[repr(u16)]
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
    UnexpectedFrame = 0,
    NoHandlerForPath,
    InternalError,
    IdInUse,
}

#[repr(u16)]
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
    UnexpectedFrame = 0,
    Cancelled,
    TimedOut,
}
