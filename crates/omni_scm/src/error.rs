use strum::{EnumDiscriminants, IntoDiscriminant};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl Error {
    pub fn no_repository_found() -> Self {
        Self(ErrorInner::NoRepositoryFound)
    }

    pub fn unsupported_scm() -> Self {
        Self(ErrorInner::UnsupportedScm)
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    #[inline(always)]
    fn from(inner: T) -> Self {
        let error = inner.into();
        Self(error)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Git2(#[from] git2::Error),

    #[error("unsupported scm")]
    UnsupportedScm,

    #[error("no repository found")]
    NoRepositoryFound,
}
