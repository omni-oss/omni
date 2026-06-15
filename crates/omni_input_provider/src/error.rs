use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use value_bag::OwnedValueBag;

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct Error(pub ErrorInner);

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
}
