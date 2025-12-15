use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::sync::oneshot::error::RecvError;

use super::frame::FrameType;

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

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(BridgeRpcErrorKind), vis(pub))]
pub(crate) enum BridgeRpcErrorInner {
    #[error("transport error")]
    Transport {
        #[new(into)]
        #[source]
        message: eyre::Report,
    },

    #[error("rpc is not running")]
    NotRunning,

    #[error("serialization error")]
    Serialization(
        #[from]
        #[source]
        rmp_serde::encode::Error,
    ),

    #[error("deserialization error: {0}")]
    Deserialization(
        #[from]
        #[source]
        rmp_serde::decode::Error,
    ),

    #[error("value conversion error")]
    ValueConversion(
        #[from]
        #[source]
        rmpv::ext::Error,
    ),

    #[error("receive error")]
    Receive(
        #[from]
        #[source]
        RecvError,
    ),

    #[error("send error")]
    Send {
        #[new(into)]
        #[source]
        error: eyre::Report,
    },

    #[error("timeout")]
    Timeout(
        #[new(into)]
        #[source]
        eyre::Report,
    ),

    #[error("probe in progress")]
    ProbeInProgress,

    #[error("unknown error")]
    Unknown(
        #[from]
        #[source]
        eyre::Report,
    ),

    #[error("missing data for frame type: {frame_type}")]
    MissingData { frame_type: FrameType },
}

pub type BridgeRpcResult<T> = Result<T, BridgeRpcError>;
