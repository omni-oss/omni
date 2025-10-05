use async_trait::async_trait;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[async_trait]
pub trait OrganizationRepository: Send + Sync + 'static {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, OrganizationRepositoryError>;

    async fn belongs_to_tenant(
        &self,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, OrganizationRepositoryError>;
}

pub type DynOrganizationRepository =
    Box<dyn OrganizationRepository + Send + Sync>;

#[derive(Debug, thiserror::Error)]
#[error("organization repository error: {inner:?}")]
pub struct OrganizationRepositoryError {
    inner: OrganizationRepositoryErrorInner,
    kind: OrganizationRepositoryErrorKind,
}

impl OrganizationRepositoryError {
    pub fn kind(&self) -> OrganizationRepositoryErrorKind {
        self.kind
    }
}

impl<T: Into<OrganizationRepositoryErrorInner>> From<T>
    for OrganizationRepositoryError
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
#[strum_discriminants(vis(pub), name(OrganizationRepositoryErrorKind))]
pub enum OrganizationRepositoryErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("tenant does not exist with code '{0}'")]
    TenantDoesNotExistByCode(String),
}
