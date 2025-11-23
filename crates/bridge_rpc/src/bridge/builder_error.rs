use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct BridgeRpcBuilderError(pub(crate) BridgeRpcBuilderErrorInner);

impl BridgeRpcBuilderError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(BridgeRpcBuilderErrorInner::Custom(eyre::Report::msg(
            message.into(),
        )))
    }
}

impl BridgeRpcBuilderError {
    #[allow(unused)]
    pub fn kind(&self) -> BridgeRpcBuilderErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<BridgeRpcBuilderErrorInner>> From<T> for BridgeRpcBuilderError {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(BridgeRpcBuilderErrorKind))]
pub(crate) enum BridgeRpcBuilderErrorInner {
    #[error(
        "Duplicate path is registered as a stream and request handler: {0}, path must be unique"
    )]
    DuplicatePath(String),

    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
