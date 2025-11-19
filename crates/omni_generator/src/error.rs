use std::path::PathBuf;

use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::action_handlers::utils::ResolveOutputPathError;

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn custom<T: Into<String>>(msg: T) -> Self {
        Self(ErrorInner::Custom(eyre::Report::msg(msg.into())))
    }
}

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

    #[error("generator '{name}' not found")]
    GeneratorNotFound { name: String },

    #[error(transparent)]
    LoadConfig(#[from] omni_file_data_serde::Error),

    #[error(transparent)]
    Tera(#[from] tera::Error),

    #[error(
        "expression '{expr}' in '{expr_name}' did not evaluate to a boolean, result: {result}"
    )]
    InvalidBooleanResult {
        #[new(into)]
        result: String,
        #[new(into)]
        expr: String,
        #[new(into)]
        expr_name: String,
    },

    #[error(transparent)]
    Prompt(#[from] omni_prompt::error::Error),

    #[error(transparent)]
    GeneratorDiscovery(#[from] omni_configuration_discovery::error::Error),

    #[error(transparent)]
    FileDiscovery(#[from] omni_discovery::error::Error),

    #[error(
        "there is already a generator with the name '{name}', generator names must be unique, config path: {path}"
    )]
    DuplicateGeneratorName { name: String, path: PathBuf },

    #[error("failed to write to path '{path}', error: {error}")]
    FailedToWriteFile {
        #[new(into)]
        path: PathBuf,
        #[source]
        #[new(into)]
        error: std::io::Error,
    },

    #[error("failed to read from path '{path}', error: {error}")]
    FailedToReadFile {
        #[new(into)]
        path: PathBuf,

        #[source]
        #[new(into)]
        error: std::io::Error,
    },

    #[error("path exists but it is not a directory: '{path}")]
    PathExistsButNotDir {
        #[new(into)]
        path: PathBuf,
    },

    #[error(transparent)]
    GenericIo(#[from] std::io::Error),

    #[error(transparent)]
    ResolveOutputPath(#[from] ResolveOutputPathError),

    #[error(transparent)]
    FromUtf8(#[from] std::string::FromUtf8Error),

    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),

    #[error(transparent)]
    Regex(#[from] regex::Error),

    #[error(transparent)]
    ChildProcess(#[from] omni_process::ChildProcessError),

    #[error("command '{command}' failed with exit code {exit_code}")]
    CommandFailed {
        #[new(into)]
        command: String,
        exit_code: u32,
    },

    #[error(transparent)]
    Expansion(#[from] env::ExpansionError),
}
