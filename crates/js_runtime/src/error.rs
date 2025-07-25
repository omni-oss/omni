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
#[error("JsRuntimeError: {repr}")]
pub struct JsRuntimeError {
    repr: JsRuntimeErrorRepr,
    kind: JsRuntimeErrorKind,
}

impl JsRuntimeError {
    pub fn kind(&self) -> JsRuntimeErrorKind {
        self.kind
    }
}

impl<T: Into<JsRuntimeErrorRepr>> From<T> for JsRuntimeError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { repr, kind }
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
