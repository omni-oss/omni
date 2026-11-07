use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

omni_codes::prefixes!("meta_source");

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ErrorInner::Custom(eyre::Report::msg(message.into())))
    }

    pub fn cycle_detected(chain: impl Into<String>) -> Self {
        Self(ErrorInner::CycleDetected(chain.into()))
    }

    pub fn depth_exceeded(max: usize, qualified_id: impl Into<String>) -> Self {
        Self(ErrorInner::DepthExceeded {
            max,
            qualified_id: qualified_id.into(),
        })
    }

    pub fn duplicate_member_id(
        bundle: Option<impl Into<String>>,
        id: impl Into<String>,
    ) -> Self {
        Self(ErrorInner::DuplicateMemberId {
            bundle: bundle.map(Into::into),
            id: id.into(),
        })
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
pub(crate) enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    /// Meta sources refer back to one of their own ancestors.
    #[error("cycle detected in meta source graph: {0}")]
    #[diagnostic(
        code = "meta_source.cycle_detected",
        hint = "remove the meta source that refers back to one of its ancestors"
    )]
    CycleDetected(#[diagnostic(detail = "chain")] String),

    /// Meta sources nest deeper than the allowed maximum.
    #[error(
        "meta source nesting exceeded the maximum depth of {max} at '{qualified_id}'"
    )]
    #[diagnostic(
        code = "meta_source.depth_exceeded",
        hint = "flatten the nested meta sources so that none nests more than {max} levels deep"
    )]
    DepthExceeded {
        #[diagnostic(detail)]
        max: usize,
        #[diagnostic(detail)]
        qualified_id: String,
    },

    /// Two members of the same bundle, or two projection sources, share an id.
    #[error("{}", duplicate_member_message(bundle.as_deref(), id))]
    #[diagnostic(
        code = "meta_source.duplicate_member_id",
        hint = "rename one of the sources with id `{id}` so that ids are unique"
    )]
    DuplicateMemberId {
        #[diagnostic(detail)]
        bundle: Option<String>,
        #[diagnostic(detail)]
        id: String,
    },
}

fn duplicate_member_message(bundle: Option<&str>, id: &str) -> String {
    match bundle {
        Some(bundle) => {
            format!("duplicate member id '{id}' in bundle '{bundle}'")
        }
        None => format!("duplicate projection source id '{id}'"),
    }
}
