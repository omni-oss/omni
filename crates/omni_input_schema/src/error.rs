use strum::{EnumDiscriminants, IntoDiscriminant as _};
use value_bag::OwnedValueBag;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(pub ErrorInner);

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub enum ErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(
        "duplicate input name: {0}, please ensure that all input names are unique"
    )]
    DuplicateInputName(String),

    #[error(transparent)]
    Tera(#[from] omni_tera::Error),

    #[error(transparent)]
    ValueBag(#[from] value_bag::Error),

    #[error(
        "value '{value}' is invalid for input {input_name}: {error_message}"
    )]
    InvalidValue {
        input_name: String,
        value: OwnedValueBag,
        error_message: String,
    },

    #[error(
        "invalid boolean expression result: \"{result}\" for expression: \"{expression}\", expected true or false"
    )]
    InvalidBooleanExpressionResult { result: String, expression: String },

    #[error(
        "input '{input_name}' has both secret=true and remember=true, which is contradictory: a secret value must not be persisted"
    )]
    SecretRememberConflict { input_name: String },

    #[error(
        "input '{input_name}' has kind '{kind:?}' which is not supported by this profile"
    )]
    UnsupportedInputKind {
        input_name: String,
        kind: crate::input::InputKind,
    },
}
