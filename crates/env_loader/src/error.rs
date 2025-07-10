use env::ParseError;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct EnvLoaderError {
    #[from]
    repr: EnvLoaderErrorRepr,
}

impl EnvLoaderError {
    pub fn unknown(msg: &str) -> Self {
        Self {
            repr: EnvLoaderErrorRepr::Unknown(eyre::eyre!(msg.to_string())),
        }
    }
}

impl From<eyre::Error> for EnvLoaderError {
    fn from(value: eyre::Error) -> Self {
        Self {
            repr: EnvLoaderErrorRepr::Unknown(value),
        }
    }
}

impl From<std::io::Error> for EnvLoaderError {
    fn from(value: std::io::Error) -> Self {
        Self {
            repr: EnvLoaderErrorRepr::Io(value),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnvLoaderErrorRepr {
    #[error("EnvLoaderErrorRepr::CantLoadCurrentDir")]
    CantLoadCurrentDir,

    #[error("EnvLoaderErrorRepr::PathDoesNotExist => {0}")]
    PathDoesNotExist(String),

    #[error("EnvLoaderErrorRepr::CantReadFile => {0}")]
    CantReadFile(String),

    #[error("EnvLoaderErrorRepr::CantLoadRootFile => {0}")]
    CantLoadRootFile(String),

    #[error("EnvLoaderErrorRepr::CantLoadStartDir => {0}")]
    CantLoadStartDir(String),

    #[error("EnvLoaderErrorRepr::CantParseEnv")]
    CantParseEnv(Vec<ParseError>),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("EnvLoaderErrorRepr::UnknownError => {0}")]
    Unknown(eyre::Error),
}
