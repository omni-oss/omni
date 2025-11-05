use std::path::PathBuf;

use derive_new::new;
use dir_walker::impls::IgnoreRealDirWalkerConfigBuilderError;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(thiserror::Error, Debug)]
#[error("{inner}")]
pub struct Error {
    #[source]
    inner: ErrorInner,
    kind: ErrorKind,
}

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(thiserror::Error, Debug, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Glob(#[from] globset::Error),

    #[error("failed to walk dir: {dir}")]
    WalkDir {
        dir: PathBuf,
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error("failed to get metadata for path: {path}")]
    FailedToGetMetadata {
        path: PathBuf,
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error("failed to get dir entry")]
    FailedToGetDirEntry {
        #[new(into)]
        #[source]
        source: eyre::Report,
    },

    #[error(transparent)]
    Unknown(
        #[new(into)]
        #[from]
        eyre::Report,
    ),

    #[error(transparent)]
    IgnoreRealDirWalkerConfigBuilderError(
        #[from] IgnoreRealDirWalkerConfigBuilderError,
    ),
}
