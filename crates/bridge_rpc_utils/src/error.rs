use bridge_rpc_core::{
    client::response::error::ResponseError,
    server::request::error::RequestError,
};
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ReadError(pub(crate) ReadErrorInner);

impl ReadError {
    pub fn custom<T: Into<eyre::Report>>(report: T) -> Self {
        Self(ReadErrorInner::Custom(report.into()))
    }
}

impl ReadError {
    #[allow(unused)]
    pub fn kind(&self) -> ReadErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ReadErrorInner>> From<T> for ReadError {
    fn from(value: T) -> Self {
        let inner = value.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(ReadErrorKind), vis(pub))]
pub(crate) enum ReadErrorInner {
    #[error(transparent)]
    Response(#[from] ResponseError),

    #[error(transparent)]
    Request(#[from] RequestError),

    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
