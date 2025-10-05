use async_trait::async_trait;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant};

#[async_trait]
pub trait WorkspaceRepository: Send + Sync + 'static {
    async fn exists_by_code(
        &self,
        workspace_code: &str,
    ) -> Result<bool, WorkspaceRepositoryError>;

    async fn belongs_to_organization(
        &self,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, WorkspaceRepositoryError>;
}

pub type DynWorkspaceRepository = Box<dyn WorkspaceRepository>;

#[derive(Debug, thiserror::Error)]
#[error("workspace repository error: {inner:?}")]
pub struct WorkspaceRepositoryError {
    inner: WorkspaceRepositoryErrorInner,
    kind: WorkspaceRepositoryErrorKind,
}

impl WorkspaceRepositoryError {
    #[allow(unused)]
    pub fn kind(&self) -> WorkspaceRepositoryErrorKind {
        self.kind
    }
}

impl<T: Into<WorkspaceRepositoryErrorInner>> From<T>
    for WorkspaceRepositoryError
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
#[strum_discriminants(vis(pub), name(WorkspaceRepositoryErrorKind))]
pub enum WorkspaceRepositoryErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("organization does not exist with code '{0}'")]
    OrganizationDoesNotExistByCode(String),
}
