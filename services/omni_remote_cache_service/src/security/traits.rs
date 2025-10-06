use async_trait::async_trait;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[async_trait]
pub trait SecurityService: Send + Sync + 'static {
    async fn is_valid(&self, token: &str)
    -> Result<bool, SecurityServiceError>;

    async fn can_access_tenant(
        &self,
        api_key: &str,
        tenant_code: &str,
    ) -> Result<bool, SecurityServiceError>;

    async fn can_access_organization(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, SecurityServiceError>;

    async fn can_access_workspace(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, SecurityServiceError>;

    async fn can_access_environment(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, SecurityServiceError>;

    async fn can_access(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
        required_scopes: &[&str],
    ) -> Result<bool, SecurityServiceError>;
}

pub type DynSecurityService = Box<dyn SecurityService>;

#[derive(Debug, thiserror::Error, new)]
#[error("Security Service Error: {inner:?}")]
pub struct SecurityServiceError {
    kind: SecurityServiceErrorKind,
    inner: SecurityServiceErrorInner,
}

impl SecurityServiceError {
    pub fn custom(inner: impl Into<eyre::Report>) -> Self {
        let inner = inner.into();
        Self {
            kind: SecurityServiceErrorKind::Custom,
            inner: inner.into(),
        }
    }
}

impl SecurityServiceError {
    #[allow(unused)]
    pub fn kind(&self) -> &SecurityServiceErrorKind {
        &self.kind
    }
}

impl<T: Into<SecurityServiceErrorInner>> From<T> for SecurityServiceError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self {
            kind: inner.discriminant(),
            inner: inner.into(),
        }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(vis(pub), name(SecurityServiceErrorKind))]
pub enum SecurityServiceErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
