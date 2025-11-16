use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct {{ vars.rust_error_name }}(pub(crate) {{ vars.rust_error_name }}Inner);

impl Error {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(Self::Inner::Custom(eyre::Report::msg(message)))
    }
}

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> {{ vars.rust_error_name }}Kind {
        self.0.discriminant()
    }
}

impl<T: Into<{{ vars.rust_error_name }}Inner>> From<T> for Error {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum {{ vars.rust_error_name }}Inner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
