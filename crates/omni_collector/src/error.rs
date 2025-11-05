use dir_walker::impls::{
    IgnoreRealDirWalkerError, RealGlobDirWalkerConfigBuilderError,
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        let error = inner.into();
        Self(error)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
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

    #[error("failed to hash project directory: {0}")]
    ProjectDirHasher(String),
}
