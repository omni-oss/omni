//! Resource-scoped identity contract for RFC 0030.
//!
//! This is preparatory: routes and providers still use `SecurityService` until
//! the header, provider, and authentication-cache migration lands together.

use std::sync::Arc;

use async_trait::async_trait;

/// Individual grants, never a wildcard. Providers must expand legacy `all`
/// themselves, excluding `Publish` unless it was explicitly granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Read,
    Write,
    List,
    Delete,
    Publish,
}

impl Scope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read:artifacts",
            Self::Write => "write:artifacts",
            Self::List => "list:artifacts",
            Self::Delete => "delete:artifacts",
            Self::Publish => "publish:artifacts",
        }
    }
}

/// Explicit set of requested or effective grants. Empty by default.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scopes(u8);

impl Scopes {
    pub fn iter(self) -> impl Iterator<Item = Scope> {
        [
            Scope::Read,
            Scope::Write,
            Scope::List,
            Scope::Delete,
            Scope::Publish,
        ]
        .into_iter()
        .filter(move |scope| self.contains(*scope))
    }

    pub fn contains(self, scope: Scope) -> bool {
        self.0 & Self::from(scope).0 != 0
    }

    pub fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl From<Scope> for Scopes {
    fn from(scope: Scope) -> Self {
        Self(1 << scope as u8)
    }
}

impl FromIterator<Scope> for Scopes {
    fn from_iter<T: IntoIterator<Item = Scope>>(iter: T) -> Self {
        Self(
            iter.into_iter()
                .fold(0, |bits, scope| bits | Self::from(scope).0),
        )
    }
}

impl<const N: usize> From<[Scope; N]> for Scopes {
    fn from(scopes: [Scope; N]) -> Self {
        scopes.into_iter().collect()
    }
}

/// Target of one request. The route must validate ownership before calling an
/// authenticator; this value neither performs nor replaces that validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Resource {
    pub tenant: String,
    pub org: String,
    pub ws: String,
    pub env: String,
}

/// Extensible without teaching downstream routes about credential formats.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CredentialKind {
    ApiKey,
    Custom(String),
}

impl CredentialKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ApiKey => "api_key",
            Self::Custom(kind) => kind,
        }
    }
}

/// Opaque public identifier supplied by the trusted authenticator, not the raw
/// credential. API-key providers will use their existing keyed `api_key_id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialId {
    kind: CredentialKind,
    id: String,
}

impl CredentialId {
    pub fn new(kind: CredentialKind, id: String) -> Result<Self, AuthError> {
        if !valid_identity_text(kind.as_str())
            || !valid_identity_text(&id)
            || matches!(&kind, CredentialKind::Custom(name) if name == "api_key")
        {
            return Err(AuthError::InvalidIdentity);
        }
        Ok(Self { kind, id })
    }

    pub fn kind(&self) -> &CredentialKind {
        &self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Authenticated identity and effective grants for exactly the requested
/// resource and wanted set. Never reuse this value for a different target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    principal: String,
    credential: CredentialId,
    scopes: Scopes,
    scope_pin: Option<String>,
}

impl Caller {
    pub fn new(
        principal: String,
        credential: CredentialId,
        scopes: Scopes,
        scope_pin: Option<String>,
    ) -> Result<Self, AuthError> {
        if !valid_identity_text(&principal)
            || scope_pin
                .as_deref()
                .is_some_and(|p| !valid_identity_text(p))
        {
            return Err(AuthError::InvalidIdentity);
        }
        Ok(Self {
            principal,
            credential,
            scopes,
            scope_pin,
        })
    }

    pub fn principal(&self) -> &str {
        &self.principal
    }

    pub fn credential(&self) -> &CredentialId {
        &self.credential
    }

    pub fn scopes(&self) -> Scopes {
        self.scopes
    }

    /// An authenticator's verified assertion, never a client-supplied label.
    /// Namespace resolution must additionally enforce the scope-label grammar.
    pub fn scope_pin(&self) -> Option<&str> {
        self.scope_pin.as_deref()
    }

    pub fn can(&self, scope: Scope) -> bool {
        self.scopes.contains(scope)
    }
}

pub(crate) fn valid_identity_text(text: &str) -> bool {
    !text.is_empty() && text.len() <= 256 && !text.chars().any(char::is_control)
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid authenticated identity")]
    InvalidIdentity,
    /// Implementers must not include credentials in reports. Routes must
    /// sanitize backend errors and fail closed with 503, not try another member.
    #[error("authentication backend unavailable")]
    Backend(
        #[source]
        #[from]
        eyre::Report,
    ),
}

/// Routes must use an `AuthenticatorChain`, even for a single member, so grants
/// are narrowed at the boundary. Individual implementations still have to
/// enforce the requested resource and scope contract.
#[async_trait]
pub trait Authenticator: Send + Sync + 'static {
    /// Grants are the subset of `wanted` held for `resource`. `None` means an
    /// unrecognized credential, not insufficient grants. A recognized identity
    /// with no grants is still `Some`; errors stop authentication. Never log the
    /// raw credential or retain it in the returned identity or an error.
    async fn authenticate(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError>;
}

/// Config-order, first-recognized-credential wins. There is no fallback after a
/// denial (a caller with insufficient grants) or a backend error.
#[derive(Default)]
pub struct AuthenticatorChain {
    members: Vec<Arc<dyn Authenticator>>,
}

impl AuthenticatorChain {
    pub fn new(members: Vec<Arc<dyn Authenticator>>) -> Self {
        Self { members }
    }
}

#[async_trait]
impl Authenticator for AuthenticatorChain {
    async fn authenticate(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        for member in &self.members {
            if let Some(mut caller) =
                member.authenticate(credential, resource, wanted).await?
            {
                // Defend the boundary even if a member returns extra grants.
                caller.scopes = caller.scopes.intersection(wanted);
                return Ok(Some(caller));
            }
        }
        Ok(None)
    }
}
