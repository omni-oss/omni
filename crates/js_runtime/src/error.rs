pub macro error {
    ($msg:expr) => {
        eyre::eyre!($msg)
    },
    ($fmt:expr, $($arg:tt)*) => {
        eyre::eyre!($fmt, $($arg)*)
    },
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct JsRuntimeError {
    #[from]
    repr: JsRuntimeErrorRepr,
}

impl From<std::io::Error> for JsRuntimeError {
    fn from(err: std::io::Error) -> Self {
        Self { repr: err.into() }
    }
}

impl From<eyre::Report> for JsRuntimeError {
    fn from(err: eyre::Report) -> Self {
        Self { repr: err.into() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JsRuntimeErrorRepr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Other(#[from] eyre::Report),
}

impl JsRuntimeErrorRepr {
    pub fn io(err: std::io::Error) -> Self {
        Self::Io(err)
    }

    pub fn other(err: eyre::Report) -> Self {
        Self::Other(err)
    }
}
