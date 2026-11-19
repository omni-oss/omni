use std::sync::Arc;

use derive_new::new;
use maps::UnorderedMap;

use crate::{
    config::ApiKeyConfiguration,
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
    security::{Authenticator, AuthenticatorChain, InMemoryAuthenticator},
    services::{DefaultValidationService, DynValidationService},
};

#[derive(Clone, new)]
pub struct ConfigBasedDependencyProvider {
    data: Arc<InMemoryDatabase>,
    api_keys: Arc<UnorderedMap<String, ApiKeyConfiguration>>,
    #[new(default)]
    api_key_id_secret: Option<String>,
}

impl ConfigBasedDependencyProvider {
    pub fn with_api_key_id_secret(mut self, secret: Option<String>) -> Self {
        self.api_key_id_secret = secret;
        self
    }
}

impl DependencyProvider for ConfigBasedDependencyProvider {
    fn authenticator(&self) -> Arc<dyn Authenticator> {
        Arc::new(AuthenticatorChain::new(vec![Arc::new(
            InMemoryAuthenticator::new(
                self.api_keys.clone(),
                self.api_key_id_secret.clone(),
            ),
        )]))
    }

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
