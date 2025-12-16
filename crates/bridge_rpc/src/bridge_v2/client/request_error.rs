use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::sync::oneshot::{self};

use super::super::{BridgeRpcError, frame};
use super::response::error::ResponseError;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct RequestError(pub(crate) RequestErrorInner);

impl RequestError {
    #[allow(unused)]
    pub fn kind(&self) -> RequestErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<RequestErrorInner>> From<T> for RequestError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(RequestErrorKind), vis(pub))]
pub(crate) enum RequestErrorInner {
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
    DataSend(#[source] eyre::Report),

    #[error("can't receive error")]
    ErrorReceive(
        #[from]
        #[source]
        oneshot::error::TryRecvError,
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

    #[error("unknown error")]
    Unknown(
        #[from]
        #[source]
        eyre::Report,
    ),

    #[error(transparent)]
    BridgeRpc {
        #[from]
        error: BridgeRpcError,
    },

    #[error("response error(call_id: {call_id}, code: {code}): {msg}", call_id = .0.id, code = .0.code, msg = .0.message)]
    ReceivedResponseErrorFrame(frame::ResponseError),

    #[error(transparent)]
    Response(#[from] ResponseError),
}

pub type RequestResult<T> = Result<T, RequestError>;
