use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::CapabilityDomain;

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn custom(message: impl Into<String>) -> Self {
        Self(ErrorInner::Custom(eyre::Report::msg(message.into())))
    }

    /// A rule uses a domain the profile does not support.
    pub fn unsupported_domain(
        profile: impl Into<String>,
        domain: CapabilityDomain,
        index: usize,
    ) -> Self {
        Self(ErrorInner::UnsupportedDomain {
            profile: profile.into(),
            domain,
            index,
        })
    }

    /// A pattern could not be understood.
    pub fn invalid_pattern(
        pattern: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self(ErrorInner::InvalidPattern {
            pattern: pattern.into(),
            reason: reason.into(),
        })
    }
}

impl Error {
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

    #[error(
        "capability #{index} uses domain `{domain}`, which the `{profile}` \
         profile does not support"
    )]
    UnsupportedDomain {
        profile: String,
        domain: CapabilityDomain,
        index: usize,
    },

    #[error("invalid capability pattern `{pattern}`: {reason}")]
    InvalidPattern { pattern: String, reason: String },
}
