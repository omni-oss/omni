use env::{EnvParseError, ExpansionError};
use strum::{EnumDiscriminants, IntoDiscriminant};

#[derive(Debug, thiserror::Error)]
#[error("{kind:?}Error: {inner}")]
pub struct EnvLoaderError {
    #[source]
    inner: EnvLoaderErrorInner,
    kind: EnvLoaderErrorKind,
}

impl EnvLoaderError {
    pub fn kind(&self) -> EnvLoaderErrorKind {
        self.kind
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(EnvLoaderErrorKind), vis(pub))]
pub(crate) enum EnvLoaderErrorInner {
    #[error("can't load current dir")]
    CantLoadCurrentDir,

    #[error("path does not exist: {0}")]
    PathDoesNotExist(String),

    #[error("can't read file: {0}")]
    CantReadFile(String),

    #[error("can't parse env: {0:?}")]
    CantParseEnv(EnvParseError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Expansion(#[from] ExpansionError),
}

impl<T: Into<EnvLoaderErrorInner>> From<T> for EnvLoaderError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}
