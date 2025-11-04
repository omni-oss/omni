use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};
use value_bag::OwnedValueBag;

#[derive(Debug, thiserror::Error, new)]
#[error("prompt error: {inner}")]
pub struct PromptError {
    #[source]
    pub(crate) inner: PromptErrrorInner,
    pub(crate) kind: PromptErrorKind,
}

impl PromptError {
    #[allow(unused)]
    pub fn kind(&self) -> PromptErrorKind {
        self.kind
    }
}

impl<T: Into<PromptErrrorInner>> From<T> for PromptError {
    fn from(inner: T) -> Self {
        let inner = inner.into();

        Self {
            kind: inner.discriminant(),
            inner,
        }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(PromptErrorKind))]
pub(crate) enum PromptErrrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Requestty(#[from] requestty::ErrorKind),

    #[error(
        "duplicate prompt name: {0}, please ensure that all prompt names are unique"
    )]
    DuplicatePromptName(String),

    #[error(transparent)]
    Tera(#[from] tera::Error),

    #[error(
        "value '{value}' is invalid for prompt {prompt_name}: {error_message}"
    )]
    InvalidValue {
        prompt_name: String,
        value: OwnedValueBag,
        error_message: String,
    },

    #[error(
        "invalid boolean expression result: \"{result}\" for expression: \"{expression}\", expected true or false"
    )]
    InvalidBooleanExpressionResult { result: String, expression: String },
}
