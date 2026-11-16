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
        Resource, Scope, Scopes,
    },
};

fn allows(set: &AllOrSpecificConfiguration, code: &str) -> bool {
    match set {
        AllOrSpecificConfiguration::All(_) => true,
        AllOrSpecificConfiguration::Specific(items) => items.contains(code),
    }
}

/// Static API-key authenticator. Invalid credentials return `None`; valid keys
/// outside their resource restrictions remain recognized but have no grants.
pub struct InMemoryAuthenticator {
    api_keys: Arc<UnorderedMap<String, ApiKeyConfiguration>>,
    api_key_id_secret: Option<String>,
}

impl InMemoryAuthenticator {
    pub fn new(
        api_keys: Arc<UnorderedMap<String, ApiKeyConfiguration>>,
        api_key_id_secret: Option<String>,
    ) -> Self {
        Self {
            api_keys,
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
        // Unknown, disabled and expired credentials are not recognized.
        let Some(config) = self.api_keys.get(credential).filter(|c| {
            c.enabled
                && c.expires_at
                    .is_none_or(|at| at > time::OffsetDateTime::now_utc())
        }) else {
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

        let in_resource = allows(&config.tenants, &resource.tenant)
            && allows(&config.organizations, &resource.org)
            && allows(&config.workspaces, &resource.ws)
            && allows(&config.environments, &resource.env);
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
