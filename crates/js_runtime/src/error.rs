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
pub struct JsRuntimeError(pub(crate) JsRuntimeErrorRepr);

impl JsRuntimeError {
    pub fn kind(&self) -> JsRuntimeErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<JsRuntimeErrorRepr>> From<T> for JsRuntimeError {
    fn from(value: T) -> Self {
        let repr = value.into();
        Self(repr)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(JsRuntimeErrorKind), vis(pub))]
pub(crate) enum JsRuntimeErrorRepr {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Unknown(#[from] eyre::Report),
}
