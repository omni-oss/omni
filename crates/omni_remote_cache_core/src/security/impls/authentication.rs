use std::sync::Arc;

use async_trait::async_trait;
use maps::UnorderedMap;

use crate::{
    config::{
        AllOrSpecificConfiguration, ApiKeyConfiguration, ScopesConfiguration,
    },
    hooks::{api_key_id, api_key_id_keyed},
    security::{
        AuthError, Authenticator, Caller, CredentialId, CredentialKind,
        InMemorySecurityService, Resource, Scope, Scopes, SecurityService,
    },
};

/// Static API-key authenticator. Invalid credentials return `None`; valid keys
/// outside their resource restrictions remain recognized but have no grants.
pub struct InMemoryAuthenticator {
    security: InMemorySecurityService,
    api_key_id_secret: Option<String>,
}

impl InMemoryAuthenticator {
    pub fn new(
        api_keys: Arc<UnorderedMap<String, ApiKeyConfiguration>>,
        api_key_id_secret: Option<String>,
    ) -> Self {
        Self {
            security: InMemorySecurityService::new(api_keys),
            api_key_id_secret,
        }
    }
}

#[async_trait]
impl Authenticator for InMemoryAuthenticator {
    async fn authenticate(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        if !self
            .security
            .is_valid(credential)
            .await
            .map_err(|e| AuthError::Backend(eyre::Report::new(e)))?
        {
            return Ok(None);
        }
        let Some(config) = self.security.api_keys.get(credential) else {
            return Ok(None);
        };
        if self.api_key_id_secret.as_deref() == Some("") {
            return Err(AuthError::InvalidIdentity);
        }
        let id = match &self.api_key_id_secret {
            Some(secret) => api_key_id_keyed(secret, credential),
            None => api_key_id(credential),
        };
        let principal = config.principal.clone().unwrap_or_else(|| id.clone());

        let in_resource = self
            .security
            .can_access_environment(
                credential,
                &resource.tenant,
                &resource.org,
                &resource.ws,
                &resource.env,
            )
            .await
            .map_err(|e| AuthError::Backend(eyre::Report::new(e)))?;
        let scopes = [
            (Scope::Read, ScopesConfiguration::ReadArtifacts),
            (Scope::Write, ScopesConfiguration::WriteArtifacts),
            (Scope::List, ScopesConfiguration::ListArtifacts),
            (Scope::Delete, ScopesConfiguration::DeleteArtifacts),
            (Scope::Publish, ScopesConfiguration::PublishArtifacts),
        ]
        .into_iter()
        .filter_map(|(scope, configured)| {
            let permitted = match &config.scopes {
                AllOrSpecificConfiguration::All(_) => scope != Scope::Publish,
                AllOrSpecificConfiguration::Specific(grants) => {
                    grants.contains(&configured)
                }
            };
            (in_resource && wanted.contains(scope) && permitted)
                .then_some(scope)
        })
        .collect();

        Caller::new(
            principal,
            CredentialId::new(CredentialKind::ApiKey, id)?,
            scopes,
            None, // API keys cannot prove a client-selected scope label.
        )
        .map(Some)
    }
}
