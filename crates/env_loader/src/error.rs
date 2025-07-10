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
    #[error("Can't load current dir")]
    CantLoadCurrentDir,

    #[error("Path does not exist: {0}")]
    PathDoesNotExist(String),

    #[error("Can't read file: {0}")]
    CantReadFile(String),

    #[error("Can't load root file: {0}")]
    CantLoadRootFile(String),

    #[error("Can't load start dir: {0}")]
    CantLoadStartDir(String),

    #[error("Can't parse env: {0:?}")]
    CantParseEnv(Vec<ParseError>),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Unkown error: {0}")]
    Unknown(eyre::Error),
}
