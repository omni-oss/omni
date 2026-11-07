use env::{EnvParseError, ExpansionError};
use omni_diagnostic::__private::serde_json::{self, json};
use strum::{EnumDiscriminants, IntoDiscriminant};

omni_codes::prefixes!("env_file");

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct EnvLoaderError(pub(crate) EnvLoaderErrorInner);

impl EnvLoaderError {
    pub fn kind(&self) -> EnvLoaderErrorKind {
        self.0.discriminant()
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(EnvLoaderErrorKind), vis(pub))]
pub(crate) enum EnvLoaderErrorInner {
    #[error("can't load current dir")]
    #[diagnostic(skip)]
    CantLoadCurrentDir,

    /// An env file path does not exist.
    #[error("path does not exist: {path}")]
    #[diagnostic(
        code = "env_file.not_found",
        hint = "check that `{path}` exists and is spelled correctly"
    )]
    PathDoesNotExist {
        #[diagnostic(detail)]
        path: String,
    },

    /// An env file exists but cannot be read.
    #[error("can't read file: {path}")]
    #[diagnostic(
        code = "env_file.read_failed",
        audience = environment,
        hint = "check that `{path}` is readable by the current user"
    )]
    CantReadFile {
        #[diagnostic(detail)]
        path: String,
    },

    /// An env file cannot be parsed.
    #[error("can't parse env: {err:?}")]
    #[diagnostic(
        code = "env_file.parse_failed",
        details = parse_failed_details,
        hint = "fix the syntax of the env file at `{path}`"
    )]
    CantParseEnv { path: String, err: EnvParseError },

    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    Expansion(#[from] ExpansionError),
}

fn parse_failed_details(e: &EnvLoaderErrorInner) -> serde_json::Value {
    match e {
        EnvLoaderErrorInner::CantParseEnv { path, err } => {
            match err.location() {
                Some((line, column)) => {
                    json!({ "path": path, "line": line, "column": column })
                }
                None => json!({ "path": path }),
            }
        }
        _ => serde_json::Value::Null,
    }
}

impl<T: Into<EnvLoaderErrorInner>> From<T> for EnvLoaderError {
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}
