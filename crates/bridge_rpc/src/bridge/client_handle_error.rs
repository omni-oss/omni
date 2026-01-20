use super::session::SessionManagerError;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct ClientHandleError(pub(crate) ClientHandleErrorInner);

impl ClientHandleError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ClientHandleErrorInner::Custom(eyre::Report::msg(
            message.into(),
        )))
    }
}

impl ClientHandleError {
    #[allow(unused)]
    pub fn kind(&self) -> ClientHandleErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ClientHandleErrorInner>> From<T> for ClientHandleError {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ClientHandleErrorKind))]
pub(crate) enum ClientHandleErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    SessionManager(#[from] SessionManagerError),

    #[error("RPC is not running")]
    NotRunning,
}

pub type ClientHandleResult<T> = Result<T, ClientHandleError>;
