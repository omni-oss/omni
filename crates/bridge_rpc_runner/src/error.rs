use strum::{EnumDiscriminants, IntoDiscriminant as _};

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
pub struct BridgeRunnerError(pub(crate) BridgeRunnerErrorRepr);

impl BridgeRunnerError {
    pub fn kind(&self) -> BridgeRunnerErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<BridgeRunnerErrorRepr>> From<T> for BridgeRunnerError {
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(BridgeRunnerErrorKind), vis(pub))]
pub(crate) enum BridgeRunnerErrorRepr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Unknown(#[from] eyre::Report),
}
