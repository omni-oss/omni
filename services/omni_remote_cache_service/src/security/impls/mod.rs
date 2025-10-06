use std::{str::FromStr, sync::Arc};

use derive_new::new;
use maps::UnorderedMap;

use crate::{
    config::{
        AllOrSpecificConfiguration, ApiKeyConfiguration, ScopesConfiguration,
    },
    security::{SecurityService, SecurityServiceError},
};

#[derive(Clone, new, PartialEq, Eq)]
pub struct InMemorySecurityService {
    api_keys: Arc<UnorderedMap<String, ApiKeyConfiguration>>,
}

#[async_trait::async_trait]
impl SecurityService for InMemorySecurityService {
    async fn is_valid(
        &self,
        api_key: &str,
    ) -> Result<bool, SecurityServiceError> {
        Ok(self.api_keys.contains_key(api_key))
    }

    async fn can_access_tenant(
        &self,
        api_key: &str,
        tenant_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self.is_valid(api_key).await? {
            return Ok(false);
        }

        let config = self
            .api_keys
            .get(api_key)
            .expect("should be able to get api key config");

        Ok(match &config.tenants {
            AllOrSpecificConfiguration::All(_) => true,
            AllOrSpecificConfiguration::Specific(items) => {
                items.contains(&tenant_code.to_string())
            }
        })
    }

    async fn can_access_organization(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self.is_valid(api_key).await? {
            return Ok(false);
        }

        if !self.can_access_tenant(api_key, tenant_code).await? {
            return Ok(false);
        }

        let config = self
            .api_keys
            .get(api_key)
            .expect("should be able to get api key config");

        Ok(match &config.organizations {
            AllOrSpecificConfiguration::All(_) => true,
            AllOrSpecificConfiguration::Specific(items) => {
                items.contains(&organization_code.to_string())
            }
        })
    }

    async fn can_access_workspace(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self.is_valid(api_key).await? {
            return Ok(false);
        }

        if !self
            .can_access_organization(api_key, tenant_code, organization_code)
            .await?
        {
            return Ok(false);
        }

        let config = self
            .api_keys
            .get(api_key)
            .expect("should be able to get api key config");

        Ok(match &config.workspaces {
            AllOrSpecificConfiguration::All(_) => true,
            AllOrSpecificConfiguration::Specific(items) => {
                items.contains(&workspace_code.to_string())
            }
        })
    }

    async fn can_access_environment(
        &self,
        api_key: &str,
        tenant_code: &str,
        organization_code: &str,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, SecurityServiceError> {
        if !self.is_valid(api_key).await? {
            return Ok(false);
        }

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

        let config = self
            .api_keys
            .get(api_key)
            .expect("should be able to get api key config");

        Ok(match &config.environments {
            AllOrSpecificConfiguration::All(_) => true,
            AllOrSpecificConfiguration::Specific(items) => {
                items.contains(&environment_code.to_string())
            }
        })
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
        if !self.is_valid(api_key).await? {
            return Ok(false);
        }

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

        let config = self
            .api_keys
            .get(api_key)
            .expect("should be able to get api key config");

        Ok(match &config.scopes {
            AllOrSpecificConfiguration::All(_) => true,
            AllOrSpecificConfiguration::Specific(items) => {
                for scope in required_scopes {
                    let scope = ScopesConfiguration::from_str(scope)
                        .map_err(SecurityServiceError::custom)?;
                    if !items.contains(&scope) {
                        return Ok(false);
                    }
                }

                true
            }
        })
    }
}
