use std::sync::Arc;

use async_trait::async_trait;

use super::{CachingConfiguration, TtlCache, cache_key};
use crate::{
    data::{
        environments::{
            DynEnvironmentRepository, EnvironmentRepository,
            EnvironmentRepositoryError,
        },
        organizations::{
            DynOrganizationRepository, OrganizationRepository,
            OrganizationRepositoryError,
        },
        tenants::{
            DynTenantRepository, TenantRepository, TenantRepositoryError,
        },
        workspaces::{
            DynWorkspaceRepository, WorkspaceRepository,
            WorkspaceRepositoryError,
        },
    },
    hooks::{Background, DynArtifactHooks, HookSettings},
    providers::DependencyProvider,
    security::{DynSecurityService, SecurityService, SecurityServiceError},
    services::{DefaultValidationService, DynValidationService},
};

/// Wraps a provider and caches its repository and security lookups.
pub struct CachingProvider {
    inner: Arc<dyn DependencyProvider>,
    tenants: Option<Arc<TtlCache>>,
    orgs: Option<Arc<TtlCache>>,
    workspaces: Option<Arc<TtlCache>>,
    environments: Option<Arc<TtlCache>>,
    security: Option<Arc<TtlCache>>,
}

impl CachingProvider {
    pub fn new(
        inner: Arc<dyn DependencyProvider>,
        config: &CachingConfiguration,
    ) -> Self {
        let repo = || {
            config
                .repositories
                .clone()
                .map(|c| Arc::new(TtlCache::new(c)))
        };
        Self {
            inner,
            tenants: repo(),
            orgs: repo(),
            workspaces: repo(),
            environments: repo(),
            security: config
                .security
                .clone()
                .map(|c| Arc::new(TtlCache::new(c))),
        }
    }
}

macro_rules! cached {
    ($cache:expr, $key:expr, $call:expr) => {
        match &$cache {
            Some(c) => c.get_or_try(cache_key(&$key), $call).await,
            None => $call.await,
        }
    };
}

struct CachedTenants(DynTenantRepository, Option<Arc<TtlCache>>);
struct CachedOrgs(DynOrganizationRepository, Option<Arc<TtlCache>>);
struct CachedWorkspaces(DynWorkspaceRepository, Option<Arc<TtlCache>>);
struct CachedEnvironments(DynEnvironmentRepository, Option<Arc<TtlCache>>);
struct CachedSecurity(DynSecurityService, Option<Arc<TtlCache>>);

#[async_trait]
impl TenantRepository for CachedTenants {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, TenantRepositoryError> {
        cached!(self.1, ["exists", code], self.0.exists_by_code(code))
    }
}

#[async_trait]
impl OrganizationRepository for CachedOrgs {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, OrganizationRepositoryError> {
        cached!(self.1, ["exists", code], self.0.exists_by_code(code))
    }

    async fn belongs_to_tenant(
        &self,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, OrganizationRepositoryError> {
        cached!(
            self.1,
            ["belongs", tenant_code, organization_code],
            self.0.belongs_to_tenant(tenant_code, organization_code)
        )
    }
}

#[async_trait]
impl WorkspaceRepository for CachedWorkspaces {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, WorkspaceRepositoryError> {
        cached!(self.1, ["exists", code], self.0.exists_by_code(code))
    }

    async fn belongs_to_organization(
        &self,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, WorkspaceRepositoryError> {
        cached!(
            self.1,
            ["belongs", organization_code, workspace_code],
            self.0
                .belongs_to_organization(organization_code, workspace_code)
        )
    }
}

#[async_trait]
impl EnvironmentRepository for CachedEnvironments {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, EnvironmentRepositoryError> {
        cached!(self.1, ["exists", code], self.0.exists_by_code(code))
    }

    async fn belongs_to_workspace(
        &self,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, EnvironmentRepositoryError> {
        cached!(
            self.1,
            ["belongs", workspace_code, environment_code],
            self.0
                .belongs_to_workspace(workspace_code, environment_code)
        )
    }
}

#[async_trait]
impl SecurityService for CachedSecurity {
    async fn is_valid(
        &self,
        token: &str,
    ) -> Result<bool, SecurityServiceError> {
        cached!(self.1, ["valid", token], self.0.is_valid(token))
    }

    async fn can_access_tenant(
        &self,
        api_key: &str,
        tenant_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        cached!(
            self.1,
            ["tenant", api_key, tenant_code],
            self.0.can_access_tenant(api_key, tenant_code)
        )
    }

    async fn can_access_organization(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        cached!(
            self.1,
            ["org", api_key, tenant_code, organization_code],
            self.0.can_access_organization(
                api_key,
                tenant_code,
                organization_code
            )
        )
    }

    async fn can_access_workspace(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        cached!(
            self.1,
            [
                "ws",
                api_key,
                tenant_code,
                organization_code,
                workspace_code
            ],
            self.0.can_access_workspace(
                api_key,
                tenant_code,
                organization_code,
                workspace_code
            )
        )
    }

    async fn can_access_environment(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        cached!(
            self.1,
            [
                "env",
                api_key,
                tenant_code,
                organization_code,
                workspace_code,
                environment_code
            ],
            self.0.can_access_environment(
                api_key,
                tenant_code,
                organization_code,
                workspace_code,
                environment_code
            )
        )
    }

    async fn can_access(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
        required_scopes: &[&str],
    ) -> Result<bool, SecurityServiceError> {
        let scopes = required_scopes.join(",");
        cached!(
            self.1,
            [
                "access",
                api_key,
                tenant_code,
                organization_code,
                workspace_code,
                environment_code,
                &scopes
            ],
            self.0.can_access(
                api_key,
                tenant_code,
                organization_code,
                workspace_code,
                environment_code,
                required_scopes
            )
        )
    }
}

impl DependencyProvider for CachingProvider {
    fn workspace_repository(&self) -> DynWorkspaceRepository {
        Box::new(CachedWorkspaces(
            self.inner.workspace_repository(),
            self.workspaces.clone(),
        ))
    }

    fn organization_repository(&self) -> DynOrganizationRepository {
        Box::new(CachedOrgs(
            self.inner.organization_repository(),
            self.orgs.clone(),
        ))
    }

    fn environment_repository(&self) -> DynEnvironmentRepository {
        Box::new(CachedEnvironments(
            self.inner.environment_repository(),
            self.environments.clone(),
        ))
    }

    fn tenant_repository(&self) -> DynTenantRepository {
        Box::new(CachedTenants(
            self.inner.tenant_repository(),
            self.tenants.clone(),
        ))
    }

    fn validation_service(&self) -> DynValidationService {
        Box::new(DefaultValidationService::new(
            self.tenant_repository(),
            self.organization_repository(),
            self.workspace_repository(),
            self.environment_repository(),
        ))
    }

    fn security_service(&self) -> DynSecurityService {
        Box::new(CachedSecurity(
            self.inner.security_service(),
            self.security.clone(),
        ))
    }

    fn hooks(&self) -> DynArtifactHooks {
        self.inner.hooks()
    }

    fn hook_settings(&self) -> HookSettings {
        self.inner.hook_settings()
    }

    fn background(&self) -> Option<Arc<dyn Background>> {
        self.inner.background()
    }
}
