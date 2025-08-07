use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct HasherError {
    inner: HasherErrorInner,
    kind: HasherErrorKind,
}

impl<T: Into<HasherErrorInner>> From<T> for HasherError {
    fn from(inner: T) -> Self {
        let error = inner.into();
        let kind = error.discriminant();
        Self { inner: error, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(HasherErrorKind))]
enum HasherErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] eyre::Report),
}
