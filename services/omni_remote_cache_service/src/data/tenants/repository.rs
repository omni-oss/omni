use async_trait::async_trait;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant};

#[async_trait]
pub trait TenantRepository: Send + Sync + 'static {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, TenantRepositoryError>;
}

pub type DynTenantRepository = Box<dyn TenantRepository + Send + Sync>;

#[derive(Debug, thiserror::Error)]
#[error("tenant repository error: {inner:?}")]
pub struct TenantRepositoryError {
    inner: TenantRepositoryErrorInner,
    kind: TenantRepositoryErrorKind,
}

impl TenantRepositoryError {
    pub fn kind(&self) -> TenantRepositoryErrorKind {
        self.kind
    }
}

impl<T: Into<TenantRepositoryErrorInner>> From<T> for TenantRepositoryError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self {
            kind: inner.discriminant(),
            inner,
        }
    }
}

#[derive(Debug, EnumDiscriminants, thiserror::Error, new)]
#[strum_discriminants(vis(pub), name(TenantRepositoryErrorKind))]
pub enum TenantRepositoryErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),
}
