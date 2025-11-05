use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error("generator error: {inner}")]
pub struct Error {
    #[source]
    pub(crate) inner: ErrorInner,
    pub(crate) kind: ErrorKind,
}

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self {
            kind: inner.discriminant(),
            inner,
        }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    LoadConfig(#[from] omni_file_data_serde::Error),

    #[error(transparent)]
    Prompt(#[from] omni_prompt::error::Error),

    #[error(transparent)]
    GeneratorDiscovery(#[from] omni_configuration_discovery::error::Error),
}
