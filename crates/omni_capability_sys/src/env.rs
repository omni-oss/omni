//! Configuration for how [`PolicyEnforcingSys`](crate::PolicyEnforcingSys)
//! treats environment-variable access.
//!
//! Environment handling is separated from filesystem gating for a concrete
//! reason: the bulk [`EnvVars`](system_traits::EnvVars) trait returns an opaque
//! `std::env::Vars` iterator that has no public constructor, so it **cannot be
//! filtered in place**. Env confinement is therefore applied when the
//! environment is *materialized* into an owned map via
//! [`EnvSnapshot::env_snapshot`](system_traits::EnvSnapshot::env_snapshot),
//! which the enforcing wrapper implements (in place of `EnvVars`) so that
//! consumers exposing the environment to a script (such as the bridge RPC env
//! service) can only ever read the policy-filtered view.

/// How environment access is confined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnvAccess {
    /// Drop every variable whose **name** the policy does not allow — i.e. any
    /// name for which the authorizer denies a `Request::Env { name }`. This is
    /// the fail-closed choice: only explicitly-allowed variables reach the
    /// script.
    #[default]
    Filter,
    /// Expose the full process environment unchanged.
    Passthrough,
}

impl EnvAccess {
    /// Whether this mode filters the environment at all.
    pub fn is_filtering(self) -> bool {
        matches!(self, EnvAccess::Filter)
    }
}
