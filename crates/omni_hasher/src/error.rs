use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct HasherError(pub(crate) HasherErrorInner);

impl HasherError {
    #[allow(unused)]
    pub fn kind(&self) -> HasherErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<HasherErrorInner>> From<T> for HasherError {
    fn from(inner: T) -> Self {
        let error = inner.into();
        Self(error)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(HasherErrorKind))]
pub(crate) enum HasherErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] eyre::Report),
}
