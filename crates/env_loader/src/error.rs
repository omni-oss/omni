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
            repr: EnvLoaderErrorRepr::UnknownError(eyre::eyre!(
                msg.to_string()
            )),
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

    #[error("EnvLoaderErrorRepr::ParseEnvError")]
    ParseEnvError(Vec<ParseError>),

    #[error("EnvLoaderErrorRepr::UnknownError => {0}")]
    UnknownError(eyre::Error),
}
