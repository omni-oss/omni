use strum::{EnumDiscriminants, IntoDiscriminant};

omni_codes::prefixes!("scm");

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    #[allow(unused)]
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }
}

impl Error {
    pub fn no_repository_found() -> Self {
        Self(ErrorInner::NoRepositoryFound)
    }

    pub fn unsupported_scm() -> Self {
        Self(ErrorInner::UnsupportedScm)
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    #[inline(always)]
    fn from(inner: T) -> Self {
        let error = inner.into();
        Self(error)
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    #[diagnostic(skip)]
    Gix(#[from] gix::Error),

    /// The repository uses a source control system omni does not support.
    #[error("unsupported scm")]
    #[diagnostic(
        code = "scm.unsupported",
        hint = "use a git repository, or turn off the scm integration"
    )]
    UnsupportedScm,

    /// No repository was found from the current directory.
    #[error("no repository found")]
    #[diagnostic(
        code = "scm.no_repository_found",
        hint = "run this inside a git repository, or drop filters that need one, such as `--affected`"
    )]
    NoRepositoryFound,
}
