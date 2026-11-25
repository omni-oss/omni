use omni_path_utils::WorkspacePath;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyCause {
    NoMatch,
    ExplicitDeny,
    Other,
}

/// View-local denial. The consumer's adapter maps its policy type into this,
/// so the crate does not depend on `omni_capabilities`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyReason {
    pub path: WorkspacePath,
    pub cause: DenyCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthDecision {
    Allow,
    Deny(DenyReason),
}

/// The policy could not be evaluated. Never an implicit allow.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("policy evaluation failed")]
pub struct AuthError;

pub trait InputAuthorizer: Send + Sync {
    fn authorize_read(
        &self,
        path: &WorkspacePath,
    ) -> Result<AuthDecision, AuthError>;
}
