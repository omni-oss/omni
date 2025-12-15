use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use tokio::sync::oneshot::{self};

use super::{frame, id::Id};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ResponseError(pub(crate) ResponseErrorInner);

impl ResponseError {
    #[allow(unused)]
    pub fn kind(&self) -> ResponseErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ResponseErrorInner>> From<T> for ResponseError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(ResponseErrorKind), vis(pub))]
pub(crate) enum ResponseErrorInner {
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
        error: super::BridgeRpcError,
    },

    #[error("response error(call_id: {call_id}, code: {code}): {msg}", call_id = .0.id, code = .0.code, msg = .0.message)]
    ResponseError(frame::ResponseError),

    #[error(
        "unexpected frame received for request id: {request_id}, expecting: {expected}, actual: {actual}",
        expected = .expected.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", "),
    )]
    UnexpectedFrame {
        request_id: Id,
        expected: Vec<frame::ResponseFrameType>,
        actual: frame::ResponseFrameType,
    },

    #[error(
        "no frame received for request id: {request_id}, expecting: {expected}",
        expected = .expected.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", "),
    )]
    NoFrame {
        request_id: Id,
        expected: Vec<frame::ResponseFrameType>,
    },
}

pub type ResponseResult<T> = Result<T, ResponseError>;
