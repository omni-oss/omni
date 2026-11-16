use async_trait::async_trait;
use bytes::Bytes;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

/// Which part of the remote cache a request reads or writes (RFC 0030).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Placement {
    /// Entries a publisher stored for everyone; writing needs `publish:artifacts`.
    Shared,
    /// This credential's own entries; the server picks the principal.
    #[default]
    Isolated,
}

impl Placement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Isolated => "isolated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, new)]
pub struct RemoteAccessArgs<'a> {
    pub api_base_url: &'a str,
    pub api_key: &'a str,
    pub tenant: &'a str,
    pub org: &'a str,
    pub ws: &'a str,
    pub env: &'a str,
    #[new(default)]
    pub placement: Placement,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, new)]
pub struct ValidateAccessResult {
    pub is_valid: bool,
    pub message: Option<String>,
    /// Scopes the server says this credential holds for the target
    /// (`read:artifacts`, ...). Advisory: the server enforces them on each
    /// request. Empty when the server did not report any.
    #[new(default)]
    pub scopes: Vec<String>,
    /// The largest artifact the server says it accepts, in bytes. Advisory and
    /// unauthenticated: use it to skip an upload, never to size an allocation.
    #[new(default)]
    pub max_artifact_size: Option<u64>,
}

#[async_trait]
pub trait RemoteCacheClient: Send + Sync + 'static {
    async fn validate_access(
        &self,
        remote: &RemoteAccessArgs,
    ) -> Result<ValidateAccessResult, RemoteCacheClientError>;

    async fn get_artifact(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
    ) -> Result<Option<Bytes>, RemoteCacheClientError>;

    async fn artifact_exists(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
    ) -> Result<bool, RemoteCacheClientError>;

    /// Upload an artifact. An existing immutable entry (`409 Conflict`) is
    /// accepted as success; other rejection statuses remain errors for the
    /// caller to report without failing task execution.
    async fn put_artifact(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
        artifact: Bytes,
    ) -> Result<(), RemoteCacheClientError>;
}

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct RemoteCacheClientError(RemoteCacheClientErrorInner);

impl RemoteCacheClientError {
    pub fn custom<T: Into<eyre::Report>>(inner: T) -> Self {
        let inner = inner.into();
        Self(RemoteCacheClientErrorInner::Custom(inner))
    }
}

impl RemoteCacheClientError {
    #[allow(unused)]
    pub fn kind(&self) -> RemoteCacheClientErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<RemoteCacheClientErrorInner>> From<T> for RemoteCacheClientError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner)
    }
}

#[derive(Debug, EnumDiscriminants, thiserror::Error, new)]
#[strum_discriminants(vis(pub), name(RemoteCacheClientErrorKind))]
enum RemoteCacheClientErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
