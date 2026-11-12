use std::sync::Arc;

use async_trait::async_trait;
use omni_remote_cache_core::data::{
    environments::{EnvironmentRepository, EnvironmentRepositoryError},
    organizations::{OrganizationRepository, OrganizationRepositoryError},
    tenants::{TenantRepository, TenantRepositoryError},
    workspaces::{WorkspaceRepository, WorkspaceRepositoryError},
};

use crate::{QueriesConfiguration, QueryExecutor};

#[derive(Clone)]
pub struct QueryRepositories {
    pub(crate) executor: Arc<dyn QueryExecutor>,
    pub(crate) queries: Arc<QueriesConfiguration>,
}

impl QueryRepositories {
    pub fn new(
        executor: Arc<dyn QueryExecutor>,
        queries: Arc<QueriesConfiguration>,
    ) -> Self {
        Self { executor, queries }
    }
}

macro_rules! run {
    ($s:expr, $q:ident, $err:ty, [$($p:expr),*]) => {
        $s.executor
            .exists(&$s.queries.$q, &[$($p),*])
            .await
            .map_err(|e| <$err>::from(eyre::Report::from(e)))
    };
}

#[async_trait]
impl TenantRepository for QueryRepositories {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, TenantRepositoryError> {
        run!(self, tenant_exists, TenantRepositoryError, [code])
    }
}

#[async_trait]
impl OrganizationRepository for QueryRepositories {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, OrganizationRepositoryError> {
        run!(
            self,
            organization_exists,
            OrganizationRepositoryError,
            [code]
        )
    }

    async fn belongs_to_tenant(
        &self,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, OrganizationRepositoryError> {
        run!(
            self,
            organization_belongs_to_tenant,
            OrganizationRepositoryError,
            [tenant_code, organization_code]
        )
    }
}

#[async_trait]
impl WorkspaceRepository for QueryRepositories {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, WorkspaceRepositoryError> {
        run!(self, workspace_exists, WorkspaceRepositoryError, [code])
    }

    async fn belongs_to_organization(
        &self,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, WorkspaceRepositoryError> {
        run!(
            self,
            workspace_belongs_to_organization,
            WorkspaceRepositoryError,
            [organization_code, workspace_code]
        )
    }
}

#[async_trait]
impl EnvironmentRepository for QueryRepositories {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, EnvironmentRepositoryError> {
        run!(self, environment_exists, EnvironmentRepositoryError, [code])
    }

    async fn belongs_to_workspace(
        &self,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, EnvironmentRepositoryError> {
        run!(
            self,
            environment_belongs_to_workspace,
            EnvironmentRepositoryError,
            [workspace_code, environment_code]
        )
    }
}
