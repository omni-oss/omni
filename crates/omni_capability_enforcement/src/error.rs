//! Errors raised while turning a [`RequiredCapabilities`] policy into concrete,
//! enforceable restrictions.
//!
//! Both variants are **fail-closed** signals: if enforcement cannot be set up
//! faithfully we refuse to run rather than silently widen access.
//!
//! [`RequiredCapabilities`]: omni_capabilities::RequiredCapabilities

use omni_capabilities::CapabilityDomain;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct EnforcementError(pub(crate) EnforcementErrorInner);

impl EnforcementError {
    /// The policy requires enforcing `domain`, but no selected backend can
    /// enforce it on the current platform.
    pub fn uncovered_domain(
        domain: CapabilityDomain,
        backends: impl Into<String>,
    ) -> Self {
        Self(EnforcementErrorInner::UncoveredDomain {
            domain,
            backends: backends.into(),
        })
    }

    /// The policy cannot be fully enforced by the selected backends, and the
    /// effective [`UnenforceablePolicy`](crate::UnenforceablePolicy) resolves to
    /// `deny` (the fail-closed default). Carries a rendered list of the
    /// unenforceable patterns.
    pub fn unenforceable(summary: impl Into<String>) -> Self {
        Self(EnforcementErrorInner::Unenforceable {
            summary: summary.into(),
        })
    }

    /// The caller asked for a [`RequireFloor`](crate::FloorStrictness::RequireFloor)
    /// stance, but one or more governed domains have no un-bypassable
    /// runtime-flag or OS-sandbox floor for the resolved runtime/platform — they
    /// would rest on the bypassable in-process broker/shim alone. Carries a
    /// rendered list of those [`FloorGap`](crate::FloorGap)s.
    pub fn no_floor(summary: impl Into<String>) -> Self {
        Self(EnforcementErrorInner::NoFloor {
            summary: summary.into(),
        })
    }

    pub fn kind(&self) -> EnforcementErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<EnforcementErrorInner>> From<T> for EnforcementError {
    fn from(inner: T) -> Self {
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(vis(pub), name(EnforcementErrorKind))]
pub(crate) enum EnforcementErrorInner {
    #[error(
        "policy requires enforcing `{domain}`, but none of the selected \
         enforcement backends ({backends}) can enforce it on this platform"
    )]
    UncoveredDomain {
        domain: CapabilityDomain,
        backends: String,
    },

    #[error(
        "policy cannot be fully enforced and the unenforceable policy is \
         `deny`:\n{summary}"
    )]
    Unenforceable { summary: String },

    #[error(
        "policy requires an un-bypassable enforcement floor, but one or more \
         governed domains have none for this runtime/platform:\n{summary}"
    )]
    NoFloor { summary: String },
}
