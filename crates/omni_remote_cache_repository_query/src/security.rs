use std::sync::Arc;

use async_trait::async_trait;
use omni_remote_cache_core::security::{SecurityService, SecurityServiceError};

use crate::{ApiKeyHashConfiguration, QueryRepositories};

/// API key checks backed by operator queries. The presented key is hashed
/// per configuration before it is bound, and is never logged.
pub struct QuerySecurityService {
    repos: QueryRepositories,
    hash: Arc<ApiKeyHashConfiguration>,
}

impl QuerySecurityService {
    pub fn new(
        repos: QueryRepositories,
        hash: Arc<ApiKeyHashConfiguration>,
    ) -> Self {
        Self { repos, hash }
    }

    async fn run(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<bool, SecurityServiceError> {
        self.repos
            .executor
            .exists(sql, params)
            .await
            .map_err(SecurityServiceError::custom)
    }
}

#[async_trait]
impl SecurityService for QuerySecurityService {
    async fn is_valid(
        &self,
        api_key: &str,
    ) -> Result<bool, SecurityServiceError> {
        let key = self.hash.apply(api_key);
        self.run(&self.repos.queries.api_key_valid, &[&key]).await
    }

    async fn can_access_tenant(
        &self,
        api_key: &str,
        tenant_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self.is_valid(api_key).await? {
            return Ok(false);
        }
        let key = self.hash.apply(api_key);
        self.run(&self.repos.queries.api_key_tenant, &[&key, tenant_code])
            .await
    }

    async fn can_access_organization(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self.can_access_tenant(api_key, tenant_code).await? {
            return Ok(false);
        }
        let key = self.hash.apply(api_key);
        self.run(
            &self.repos.queries.api_key_organization,
            &[&key, tenant_code, organization_code],
        )
        .await
    }

    async fn can_access_workspace(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self
            .can_access_organization(api_key, tenant_code, organization_code)
            .await?
        {
            return Ok(false);
        }
        let key = self.hash.apply(api_key);
        self.run(
            &self.repos.queries.api_key_workspace,
            &[&key, tenant_code, organization_code, workspace_code],
        )
        .await
    }

    async fn can_access_environment(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self
            .can_access_workspace(
                api_key,
                tenant_code,
                organization_code,
                workspace_code,
            )
            .await?
        {
            return Ok(false);
        }
        let key = self.hash.apply(api_key);
        self.run(
            &self.repos.queries.api_key_environment,
            &[
                &key,
                tenant_code,
                organization_code,
                workspace_code,
                environment_code,
            ],
        )
        .await
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
        if !self
            .can_access_environment(
                api_key,
                tenant_code,
                organization_code,
                workspace_code,
                environment_code,
            )
            .await?
        {
            return Ok(false);
        }
        let key = self.hash.apply(api_key);
        for scope in required_scopes {
            if !self
                .run(&self.repos.queries.api_key_scope, &[&key, scope])
                .await?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
