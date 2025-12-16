use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use super::super::{frame, id::Id};

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
    #[error("response error(call_id: {call_id}, code: {code}): {msg}", call_id = .0.id, code = .0.code, msg = .0.message)]
    ResponseError(frame::ResponseError),

    #[error(
        "unexpected frame received for request id: {request_id}, expecting: {expected}, actual: {actual}",
        expected = .expected.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", "),
    )]
    UnexpectedFrame {
        request_id: Id,
        expected: Vec<frame::ChannelRequestFrameType>,
        actual: frame::ChannelRequestFrameType,
    },

    #[error(
        "no frame received for request id: {request_id}, expecting: {expected}",
        expected = .expected.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", "),
    )]
    NoFrame {
        request_id: Id,
        expected: Vec<frame::ChannelRequestFrameType>,
    },

    #[error("unknown error")]
    Unknown(
        #[from]
        #[source]
        eyre::Report,
    ),
}

pub type RequestResult<T> = Result<T, RequestError>;
