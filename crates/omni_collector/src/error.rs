use dir_walker::impls::{
    IgnoreRealDirWalkerError, RealGlobDirWalkerConfigBuilderError,
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct Error {
    inner: ErrorInner,
    kind: ErrorKind,
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        let error = inner.into();
        let kind = error.discriminant();
        Self { inner: error, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
enum ErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error(transparent)]
    Globset(#[from] globset::Error),

    #[error(transparent)]
    Ignore(#[from] dir_walker::impls::IgnoreError),

    #[error(transparent)]
    RealGlobGlobDirWalkerConfigBuilder(
        #[from] RealGlobDirWalkerConfigBuilderError,
    ),

    #[error(transparent)]
    IgnoreRealDirWalker(#[from] IgnoreRealDirWalkerError),
}
