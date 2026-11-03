use async_trait::async_trait;
use derive_new::new;

use crate::{
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    services::{
        ValidationResult, ValidationService, ValidationServiceError, Violation,
    },
};

#[derive(new)]
pub struct DefaultValidationService {
    tenant_repository: DynTenantRepository,
    organization_repository: DynOrganizationRepository,
    workspace_repository: DynWorkspaceRepository,
    environment_repository: DynEnvironmentRepository,
}

#[async_trait]
impl ValidationService for DefaultValidationService {
    async fn validate_ownership(
        &self,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<ValidationResult, ValidationServiceError> {
        let mut violations = Vec::new();

        if !self.tenant_repository.exists_by_code(tenant_code).await? {
            violations.push(Violation::TenantDoesNotExist);
        }

        if !self
            .organization_repository
            .exists_by_code(organization_code)
            .await?
        {
            violations.push(Violation::OrganizationDoesNotExist);
        }

        if !self
            .workspace_repository
            .exists_by_code(workspace_code)
            .await?
        {
            violations.push(Violation::WorkspaceDoesNotExist);
        }

        if !self
            .environment_repository
            .exists_by_code(environment_code)
            .await?
        {
            violations.push(Violation::EnvironmentDoesNotExist);
        }

        if !violations.is_empty() {
            return Ok(ValidationResult::new(violations));
        }

        if !self
            .organization_repository
            .belongs_to_tenant(tenant_code, organization_code)
            .await?
        {
            violations.push(Violation::TenantDoesNotHaveOrganization);
        }

        if !self
            .workspace_repository
            .belongs_to_organization(organization_code, workspace_code)
            .await?
        {
            violations.push(Violation::OrganizationDoesNotHaveWorkspace);
        }

        if !self
            .environment_repository
            .belongs_to_workspace(workspace_code, environment_code)
            .await?
        {
            violations.push(Violation::WorkspaceDoesNotHaveEnvironment);
        }

        Ok(ValidationResult::new(violations))
    }
}
