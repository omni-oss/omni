use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use super::super::{super::Id, frame};

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
    #[error("response error(response_id: {response_id}, code: {code}): {msg}", response_id = .0.id, code = .0.code, msg = .0.message)]
    Response(frame::ResponseError),

    #[error(
        "failed to receive response start frame (response_id: {response_id})"
    )]
    FailedToReceiveResponseStartFrame { response_id: Id },

    #[error("trailers are not available when response body is not fully read")]
    TrailersNotAvailable,
}

pub type ResponseResult<T> = Result<T, ResponseError>;
