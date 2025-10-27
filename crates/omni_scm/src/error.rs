use strum::{EnumDiscriminants, IntoDiscriminant};

#[derive(Debug, thiserror::Error)]
#[error("ScmError: {inner}")]
pub struct Error {
    inner: ErrorInner,
    kind: ErrorKind,
}

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl Error {
    pub fn no_repository_found() -> Self {
        Self {
            inner: ErrorInner::NoRepositoryFound,
            kind: ErrorKind::NoRepositoryFound,
        }
    }

    pub fn unsupported_scm() -> Self {
        Self {
            inner: ErrorInner::UnsupportedScm,
            kind: ErrorKind::UnsupportedScm,
        }
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    #[inline(always)]
    fn from(inner: T) -> Self {
        let error = inner.into();
        let kind = error.discriminant();
        Self { inner: error, kind }
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
