use async_trait::async_trait;
use bytes::Bytes;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, new)]
pub struct RemoteAccessArgs<'a> {
    pub endpoint_base_url: &'a str,
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
pub trait RemoteCacheServiceClient: Send + Sync + 'static {
    async fn validate_access(
        &self,
        remote: &RemoteAccessArgs,
    ) -> Result<ValidateAccessResult, RemoteCacheServiceClientError>;

    async fn get_artifact(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
    ) -> Result<Option<Bytes>, RemoteCacheServiceClientError>;

    async fn artifact_exists(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
    ) -> Result<bool, RemoteCacheServiceClientError>;

    async fn put_artifact(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
        artifact: Bytes,
    ) -> Result<(), RemoteCacheServiceClientError>;
}

#[derive(Debug, thiserror::Error, new)]
#[error("RemoteCacheServiceClientError: {inner:?}")]
pub struct RemoteCacheServiceClientError {
    inner: RemoteCacheServiceClientErrorInner,
    kind: RemoteCacheServiceClientErrorKind,
}

impl RemoteCacheServiceClientError {
    pub fn custom<T: Into<eyre::Report>>(inner: T) -> Self {
        let inner = inner.into();
        Self {
            inner: RemoteCacheServiceClientErrorInner::Custom(inner),
            kind: RemoteCacheServiceClientErrorKind::Custom,
        }
    }
}

impl RemoteCacheServiceClientError {
    #[allow(unused)]
    pub fn kind(&self) -> RemoteCacheServiceClientErrorKind {
        self.kind
    }
}

impl<T: Into<RemoteCacheServiceClientErrorInner>> From<T>
    for RemoteCacheServiceClientError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self {
            kind: inner.discriminant(),
            inner,
        }
    }
}

#[derive(Debug, EnumDiscriminants, thiserror::Error, new)]
#[strum_discriminants(vis(pub), name(RemoteCacheServiceClientErrorKind))]
enum RemoteCacheServiceClientErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
