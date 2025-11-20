use std::fmt::Display;

use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::sync::oneshot::error::RecvError;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct BridgeRpcError(pub(crate) BridgeRpcErrorInner);

impl BridgeRpcError {
    #[allow(unused)]
    pub fn kind(&self) -> BridgeRpcErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<BridgeRpcErrorInner>> From<T> for BridgeRpcError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(BridgeRpcErrorKind), vis(pub))]
pub(crate) enum BridgeRpcErrorInner {
    #[error("transport error: {message}")]
    Transport { message: String },

    #[error("serialization error: {0}")]
    Serialization(#[from] rmp_serde::encode::Error),

    #[error("deserialization error: {0}")]
    Deserialization(#[from] rmp_serde::decode::Error),

    #[error("value conversion error: {0}")]
    ValueConversion(#[from] rmpv::ext::Error),

    #[error("receive error: {0}")]
    Receive(#[from] RecvError),

    #[error("send error: {message}")]
    Send { message: String },

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("probe in progress")]
    ProbeInProgress,

    #[error("unknown error: {0}")]
    Unknown(#[from] eyre::Report),
}

impl BridgeRpcErrorInner {
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

pub type BridgeRpcResult<T> = Result<T, BridgeRpcError>;
