use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ErrorInner::Custom(eyre::Report::msg(message.into())))
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
    Io(#[from] std::io::Error),

    #[error(transparent)]
    BridgeRpc(#[from] bridge_rpc_core::BridgeRpcError),

    #[error("the spawned child process produced no {stream}")]
    MissingChildStream { stream: &'static str },

    #[error(transparent)]
    Custom(#[from] eyre::Report),
}

impl Error {
    pub(crate) fn missing_child_stream(stream: &'static str) -> Self {
        Self(ErrorInner::MissingChildStream { stream })
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
