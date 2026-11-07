use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::CapabilityDomain;

omni_codes::prefixes!("capability_policy");

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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

    /// A `deny` rule set `on_unenforceable: allow`, which would silently drop the
    /// deny when it cannot be enforced — a fail-**open** on a security control.
    pub fn deny_cannot_silence_unenforceable(index: usize) -> Self {
        Self(ErrorInner::DenyCannotSilenceUnenforceable { index })
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

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, new, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(ErrorKind))]
pub(crate) enum ErrorInner {
    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    /// A rule uses a domain the profile does not support.
    #[error(
        "capability #{index} uses domain `{domain}`, which the `{profile}` \
         profile does not support"
    )]
    #[diagnostic(
        code = "capability_policy.unsupported_domain",
        hint = "remove capability #{index} or use a profile that supports `{domain}`"
    )]
    UnsupportedDomain {
        #[diagnostic(detail)]
        profile: String,
        #[diagnostic(detail)]
        domain: CapabilityDomain,
        #[diagnostic(detail)]
        index: usize,
    },

    /// A capability pattern could not be parsed.
    #[error("invalid capability pattern `{pattern}`: {reason}")]
    #[diagnostic(
        code = "capability_policy.invalid_pattern",
        hint = "correct the capability pattern so it parses"
    )]
    InvalidPattern { pattern: String, reason: String },

    /// A `deny` rule would be silently dropped when it cannot be enforced.
    #[error(
        "capability #{index} is a `deny` rule with `on_unenforceable: allow`; a \
         deny may not be silently dropped when it cannot be enforced (use \
         `warn` or `deny`)"
    )]
    #[diagnostic(
        code = "capability_policy.deny_cannot_silence_unenforceable",
        hint = "set `on_unenforceable` to `warn` or `deny` on capability #{index}"
    )]
    DenyCannotSilenceUnenforceable {
        #[diagnostic(detail)]
        index: usize,
    },
}
