use std::fmt::Display;

use tokio::sync::oneshot::error::RecvError;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct JsBridgeError(#[from] JsBridgeErrorInner);

#[derive(Debug, thiserror::Error)]
pub enum JsBridgeErrorInner {
    #[error("Transport error: {message}")]
    Transport { message: String },

    #[error("Serialization error: {0}")]
    Serialization(#[from] rmp_serde::encode::Error),

    #[error("Deserialization error: {0}")]
    Deserialization(#[from] rmp_serde::decode::Error),

    #[error("Value conversion error: {0}")]
    ValueConversion(#[from] rmpv::ext::Error),

    #[error("Receive error: {0}")]
    Receive(#[from] RecvError),

    #[error("Send error: {message}")]
    Send { message: String },

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Probe in progress")]
    ProbeInProgress,

    #[error("Unknown error: {0}")]
    Unknown(#[from] eyre::Report),
}

impl JsBridgeErrorInner {
    pub(crate) fn transport(error: impl Display) -> Self {
        Self::Transport {
            message: error.to_string(),
        }
    }

    pub(crate) fn send(message: impl Into<String>) -> Self {
        Self::Send {
            message: message.into(),
        }
    }
}

pub type JsBridgeResult<T> = Result<T, JsBridgeError>;
