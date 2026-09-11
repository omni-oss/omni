use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
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

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("cycle detected in meta source graph: {0}")]
    CycleDetected(String),

    #[error(
        "meta source nesting exceeded the maximum depth of {max} at '{qualified_id}'"
    )]
    DepthExceeded { max: usize, qualified_id: String },

    #[error("{}", duplicate_member_message(bundle.as_deref(), id))]
    DuplicateMemberId { bundle: Option<String>, id: String },
}

fn duplicate_member_message(bundle: Option<&str>, id: &str) -> String {
    match bundle {
        Some(bundle) => {
            format!("duplicate member id '{id}' in bundle '{bundle}'")
        }
        None => format!("duplicate projection source id '{id}'"),
    }
}
