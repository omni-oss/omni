use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use super::super::frame;

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
    #[error("request error(call_id: {call_id}, code: {code}): {msg}", call_id = .0.id, code = .0.code, msg = .0.message)]
    RequestError(frame::RequestError),

    #[error("unknown error")]
    Unknown(
        #[from]
        #[source]
        eyre::Report,
    ),

    #[error("trailers are not available when request body is not fully read")]
    TrailersNotAvailable,
}

pub type RequestResult<T> = Result<T, RequestError>;
