use std::path::PathBuf;

use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
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
        let inner = inner.into();

        Self(inner)
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

    #[error(
        "there is already a generator with the name '{name}', generator names must be unique, config path: {path}"
    )]
    DuplicateGeneratorName { name: String, path: PathBuf },
}
