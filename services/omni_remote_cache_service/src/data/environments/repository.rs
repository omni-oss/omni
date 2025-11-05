use async_trait::async_trait;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

#[async_trait]
pub trait EnvironmentRepository: Send + Sync + 'static {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, EnvironmentRepositoryError>;

    async fn belongs_to_workspace(
        &self,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, EnvironmentRepositoryError>;
}

pub type DynEnvironmentRepository =
    Box<dyn EnvironmentRepository + Send + Sync>;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct EnvironmentRepositoryError(
    pub(crate) EnvironmentRepositoryErrorInner,
);

impl EnvironmentRepositoryError {
    #[allow(unused)]
    pub fn kind(&self) -> EnvironmentRepositoryErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<EnvironmentRepositoryErrorInner>> From<T>
    for EnvironmentRepositoryError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner)
    }
}

#[derive(Debug, EnumDiscriminants, thiserror::Error, new)]
#[strum_discriminants(vis(pub), name(EnvironmentRepositoryErrorKind))]
pub(crate) enum EnvironmentRepositoryErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("workspace does not exist with code '{0}'")]
    WorkspaceDoesNotExistByCode(String),
}
