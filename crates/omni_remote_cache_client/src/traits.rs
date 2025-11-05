use async_trait::async_trait;
use bytes::Bytes;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, new)]
pub struct RemoteAccessArgs<'a> {
    pub api_base_url: &'a str,
    pub api_key: &'a str,
    pub tenant: &'a str,
    pub org: &'a str,
    pub ws: &'a str,
    pub env: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, new)]
pub struct ValidateAccessResult {
    pub is_valid: bool,
    pub message: Option<String>,
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
