use std::path::PathBuf;

use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct Error {
    #[source]
    inner: ErrorInner,
    kind: ErrorKind,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs)]
#[strum_discriminants(
    name(ErrorKind),
    vis(pub),
    derive(strum::IntoStaticStr, strum::Display, strum::EnumIs)
)]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("failed to find workspace configuration")]
    FailedToFindWorkspaceConfiguration,

    #[error("failed to load workspace configuration: '{0}'")]
    FailedToLoadWorkspaceConfiguration(PathBuf, #[source] serde_yml::Error),
}
