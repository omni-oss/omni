use std::sync::Arc;

use derive_new::new;

use crate::{
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    data_impl::in_memory::{
        InMemoryDatabase, InMemoryEnvironmentRepository,
        InMemoryOrganizationRepository, InMemoryTenantRepository,
        InMemoryWorkspaceRepository,
    },
    providers::DependencyProvider,
    services::{DefaultValidationService, DynValidationService},
};

#[derive(Clone, new)]
pub struct InMemoryDependencyProvider {
    data: Arc<InMemoryDatabase>,
}

impl DependencyProvider for InMemoryDependencyProvider {
    fn workspace_repository(&self) -> DynWorkspaceRepository {
        Box::new(InMemoryWorkspaceRepository::new(self.data.clone()))
    }

    fn organization_repository(&self) -> DynOrganizationRepository {
        Box::new(InMemoryOrganizationRepository::new(self.data.clone()))
    }

    fn environment_repository(&self) -> DynEnvironmentRepository {
        Box::new(InMemoryEnvironmentRepository::new(self.data.clone()))
    }

    fn tenant_repository(&self) -> DynTenantRepository {
        Box::new(InMemoryTenantRepository::new(self.data.clone()))
    }

    fn validation_service(&self) -> DynValidationService {
        Box::new(DefaultValidationService::new(
            self.tenant_repository(),
            self.organization_repository(),
            self.workspace_repository(),
            self.environment_repository(),
        ))
    }
}
