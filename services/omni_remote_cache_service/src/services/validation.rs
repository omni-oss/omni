use async_trait::async_trait;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant};

use crate::data::{
    environments::EnvironmentRepositoryError,
    organizations::OrganizationRepositoryError, tenants::TenantRepositoryError,
    workspaces::WorkspaceRepositoryError,
};

#[async_trait]
pub trait ValidationService: Send + Sync + 'static {
    async fn validate_ownership(
        &self,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<ValidationResult, ValidationServiceError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, new)]
pub struct ValidationResult {
    #[new(into)]
    violations: Vec<Violation>,
}

impl ValidationResult {
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Violation {
    TenantDoesNotExist,
    TenantDoesNotHaveOrganization,
    OrganizationDoesNotExist,
    OrganizationDoesNotHaveWorkspace,
    WorkspaceDoesNotExist,
    WorkspaceDoesNotHaveEnvironment,
    EnvironmentDoesNotExist,
}

#[derive(Debug, thiserror::Error)]
#[error("validation service error: {inner:?}")]
pub struct ValidationServiceError {
    inner: ValidationServiceErrorInner,
    kind: ValidationServiceErrorKind,
}

pub type DynValidationService = Box<dyn ValidationService + Send + Sync>;

impl ValidationServiceError {
    pub fn kind(&self) -> ValidationServiceErrorKind {
        self.kind
    }
}

impl<T: Into<ValidationServiceErrorInner>> From<T> for ValidationServiceError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self {
            kind: inner.discriminant(),
            inner,
        }
    }
}

#[derive(Debug, EnumDiscriminants, thiserror::Error, new)]
#[strum_discriminants(vis(pub), name(ValidationServiceErrorKind))]
pub enum ValidationServiceErrorInner {
    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    TenantRepository(#[from] TenantRepositoryError),

    #[error(transparent)]
    OrganizationRepository(#[from] OrganizationRepositoryError),

    #[error(transparent)]
    WorkspaceRepository(#[from] WorkspaceRepositoryError),

    #[error(transparent)]
    EnvironmentRepository(#[from] EnvironmentRepositoryError),
}
