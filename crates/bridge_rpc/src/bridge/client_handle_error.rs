use super::session::SessionManagerError;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct BridgeRpcClientHandleError(
    pub(crate) BridgeRpcClientHandleErrorInner,
);

impl BridgeRpcClientHandleError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(BridgeRpcClientHandleErrorInner::Custom(eyre::Report::msg(
            message.into(),
        )))
    }
}

impl BridgeRpcClientHandleError {
    #[allow(unused)]
    pub fn kind(&self) -> BridgeRpcClientHandleErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<BridgeRpcClientHandleErrorInner>> From<T>
    for BridgeRpcClientHandleError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(BridgeRpcClientHandleErrorKind))]
pub(crate) enum BridgeRpcClientHandleErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    SessionManager(#[from] SessionManagerError),

    #[error("RPC is not running")]
    NotRunning,
}

pub type BridgeRpcClientHandleResult<T> = Result<T, BridgeRpcClientHandleError>;
