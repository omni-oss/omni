use crate::{
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    services::DynValidationService,
};

pub trait DependencyProvider: Send + Sync + 'static {
    fn workspace_repository(&self) -> DynWorkspaceRepository;
    fn organization_repository(&self) -> DynOrganizationRepository;
    fn environment_repository(&self) -> DynEnvironmentRepository;
    fn tenant_repository(&self) -> DynTenantRepository;
    fn validation_service(&self) -> DynValidationService;
}
