use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

omni_codes::prefixes!("input_provider");

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub ErrorInner);

impl Error {
    pub fn custom(report: eyre::Report) -> Self {
        Self(ErrorInner::Custom(report))
    }

    pub fn invalid_value_message(&self) -> Option<&str> {
        match &self.0 {
            ErrorInner::Schema(schema) => match &schema.0 {
                omni_input_schema::error::ErrorInner::InvalidValue {
                    error_message,
                    ..
                } => Some(error_message),
                _ => None,
            },
            _ => None,
        }
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

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, new, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Tera(#[from] omni_tera::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    ValueBag(#[from] value_bag::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Schema(#[from] omni_input_schema::Error),
}
