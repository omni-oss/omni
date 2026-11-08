use strum::{EnumDiscriminants, IntoDiscriminant as _};
use value_bag::OwnedValueBag;

omni_codes::prefixes!("input");

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    /// Two inputs share the same name.
    #[error(
        "duplicate input name: {0}, please ensure that all input names are unique"
    )]
    #[diagnostic(
        code = "input.duplicate_name",
        hint = "rename one of the inputs named `{0}` so input names are unique"
    )]
    DuplicateInputName(#[diagnostic(detail = "input_name")] String),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Tera(#[from] omni_tera::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ValueBag(#[from] value_bag::Error),

    /// A value does not pass the validators of its input.
    // The value is deliberately not in the message: it may be a secret.
    #[error("value is invalid for input {input_name}: {error_message}")]
    #[diagnostic(
        code = "input.invalid_value",
        hint = "provide a value for input `{input_name}` that passes its validators"
    )]
    InvalidValue {
        #[diagnostic(detail)]
        input_name: String,
        value: OwnedValueBag,
        error_message: String,
    },

    /// A boolean expression did not evaluate to `true` or `false`.
    // The expression and its result are deliberately not in the message: a
    // validator over a secret input renders the secret into the result.
    #[error("invalid boolean expression result: expected true or false")]
    #[diagnostic(
        code = "input.invalid_boolean_result",
        hint = "make the expression evaluate to `true` or `false`"
    )]
    InvalidBooleanExpressionResult { result: String, expression: String },

    /// An input is marked both `secret` and `remember`.
    #[error(
        "input '{input_name}' has both secret=true and remember=true, which is contradictory: a secret value must not be persisted"
    )]
    #[diagnostic(
        code = "input.secret_remember_conflict",
        hint = "remove `secret` or `remember` from input `{input_name}`; a secret value must not be persisted"
    )]
    SecretRememberConflict {
        #[diagnostic(detail)]
        input_name: String,
    },

    /// An input uses a kind the profile does not support.
    #[error(
        "input '{input_name}' has kind '{kind:?}' which is not supported by this profile"
    )]
    #[diagnostic(
        code = "input.unsupported_kind",
        hint = "change input `{input_name}` to a kind this profile supports"
    )]
    UnsupportedInputKind {
        #[diagnostic(detail)]
        input_name: String,
        #[diagnostic(detail)]
        kind: crate::input::InputKind,
    },
}
