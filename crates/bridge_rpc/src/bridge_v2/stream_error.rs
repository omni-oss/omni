use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct StreamError(pub(crate) StreamErrorInner);

impl StreamError {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(StreamErrorInner::Custom(eyre::Report::msg(message.into())))
    }
}

impl StreamError {
    #[allow(unused)]
    pub fn kind(&self) -> StreamErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<StreamErrorInner>> From<T> for StreamError {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(StreamErrorKind))]
pub(crate) enum StreamErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("value conversion error")]
    ValueConversion(
        #[from]
        #[source]
        rmpv::ext::Error,
    ),
}
