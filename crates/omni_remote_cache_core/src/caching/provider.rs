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
    hooks::{DynArtifactHooks, HookSettings, HostRuntime},
    providers::DependencyProvider,
    security::{DynSecurityService, SecurityService, SecurityServiceError},
    services::{DefaultValidationService, DynValidationService},
};

/// The caches themselves. Share one across providers that are rebuilt per
/// request (as on Workers) so entries outlive a single request.
pub struct CachingState {
    tenants: Option<Arc<TtlCache>>,
    orgs: Option<Arc<TtlCache>>,
    workspaces: Option<Arc<TtlCache>>,
    environments: Option<Arc<TtlCache>>,
    security: Option<Arc<TtlCache>>,
}

impl CachingState {
    pub fn new(config: &CachingConfiguration) -> Self {
        let repo = || {
            config
                .repositories
                .clone()
                .map(|c| Arc::new(TtlCache::new(c)))
        };
        Self {
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

/// Wraps a provider and caches its repository and security lookups.
///
/// Ownership validation is rebuilt from the cached repositories, so a wrapped
/// provider that overrides `validation_service` with extra rules loses them
/// here. Wrap providers whose validation is the default repository check.
pub struct CachingProvider {
    inner: Arc<dyn DependencyProvider>,
    state: Arc<CachingState>,
}

impl CachingProvider {
    pub fn new(
        inner: Arc<dyn DependencyProvider>,
        config: &CachingConfiguration,
    ) -> Self {
        Self::with_state(inner, Arc::new(CachingState::new(config)))
    }

    pub fn with_state(
        inner: Arc<dyn DependencyProvider>,
        state: Arc<CachingState>,
    ) -> Self {
        Self { inner, state }
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
        // One key part per scope, so scope lists cannot collide.
        let mut key = vec![
            "access",
            api_key,
            tenant_code,
            organization_code,
            workspace_code,
            environment_code,
        ];
        key.extend(required_scopes.iter().copied());
        cached!(
            self.1,
            key,
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
            self.state.workspaces.clone(),
        ))
    }

    fn organization_repository(&self) -> DynOrganizationRepository {
        Box::new(CachedOrgs(
            self.inner.organization_repository(),
            self.state.orgs.clone(),
        ))
    }

    fn environment_repository(&self) -> DynEnvironmentRepository {
        Box::new(CachedEnvironments(
            self.inner.environment_repository(),
            self.state.environments.clone(),
        ))
    }

    fn tenant_repository(&self) -> DynTenantRepository {
        Box::new(CachedTenants(
            self.inner.tenant_repository(),
            self.state.tenants.clone(),
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
            self.state.security.clone(),
        ))
    }

    fn hooks(&self) -> DynArtifactHooks {
        self.inner.hooks()
    }

    fn hook_settings(&self) -> HookSettings {
        self.inner.hook_settings()
    }

    fn runtime(&self) -> Option<Arc<dyn HostRuntime>> {
        self.inner.runtime()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::caching::CacheConfiguration;

    struct Counting(Arc<AtomicUsize>);

    #[async_trait]
    impl SecurityService for Counting {
        async fn is_valid(
            &self,
            _: &str,
        ) -> Result<bool, SecurityServiceError> {
            Ok(true)
        }
        async fn can_access_tenant(
            &self,
            _: &str,
            _: &str,
        ) -> Result<bool, SecurityServiceError> {
            Ok(true)
        }
        async fn can_access_organization(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<bool, SecurityServiceError> {
            Ok(true)
        }
        async fn can_access_workspace(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<bool, SecurityServiceError> {
            Ok(true)
        }
        async fn can_access_environment(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<bool, SecurityServiceError> {
            Ok(true)
        }
        async fn can_access(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &[&str],
        ) -> Result<bool, SecurityServiceError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
    }

    #[tokio::test]
    async fn different_scope_lists_never_share_a_cache_entry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let svc = CachedSecurity(
            Box::new(Counting(calls.clone())),
            Some(Arc::new(TtlCache::new(CacheConfiguration {
                ttl_secs: 60,
                max_entries: 100,
                negative_ttl_secs: None,
            }))),
        );
        svc.can_access("k", "t", "o", "w", "e", &["a,b", "c"])
            .await
            .unwrap();
        svc.can_access("k", "t", "o", "w", "e", &["a", "b,c"])
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        svc.can_access("k", "t", "o", "w", "e", &["a", "b,c"])
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2, "same list is cached");
    }
}
