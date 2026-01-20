use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct HandlerError(pub(crate) HandlerErrorInner);

impl HandlerError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(HandlerErrorInner::Custom(eyre::Report::msg(message.into())))
    }

    pub fn custom_error(error: impl Into<eyre::Report>) -> Self {
        Self(HandlerErrorInner::Custom(error.into()))
    }
}

impl HandlerError {
    #[allow(unused)]
    pub fn kind(&self) -> HandlerErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<HandlerErrorInner>> From<T> for HandlerError {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(HandlerErrorKind))]
pub(crate) enum HandlerErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
