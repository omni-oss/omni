use std::sync::Arc;

use async_trait::async_trait;
use omni_remote_cache_core::{
    hooks::{api_key_id, api_key_id_keyed},
    security::{
        AuthError, Authenticator, Caller, CredentialId, CredentialKind,
        Resource, Scope, Scopes,
    },
};

use crate::{ApiKeyHashConfiguration, QueryRepositories};

/// Query-backed API-key identity. Hashes the credential once, reads the optional
/// principal with validity, then checks the resource and only requested scopes.
pub struct QueryAuthenticator {
    repos: QueryRepositories,
    hash: Arc<ApiKeyHashConfiguration>,
    api_key_id_secret: Option<String>,
}

impl QueryAuthenticator {
    pub fn new(
        repos: QueryRepositories,
        hash: Arc<ApiKeyHashConfiguration>,
        api_key_id_secret: Option<String>,
    ) -> Self {
        Self {
            repos,
            hash,
            api_key_id_secret,
        }
    }

    async fn exists(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<bool, AuthError> {
        self.repos
            .executor
            .exists(sql, params)
            .await
            .map_err(|e| AuthError::Backend(e.into()))
    }
}

#[async_trait]
impl Authenticator for QueryAuthenticator {
    async fn authenticate(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        let key = self.hash.apply(credential);
        let Some(identity) = self
            .repos
            .executor
            .api_key_identity(&self.repos.queries.api_key_valid, &[&key])
            .await
            .map_err(|e| AuthError::Backend(e.into()))?
        else {
            return Ok(None);
        };
        if !identity.found {
            return Ok(None);
        }
        let id = match &self.api_key_id_secret {
            Some(secret) if secret.is_empty() => {
                return Err(AuthError::InvalidIdentity);
            }
            Some(secret) => api_key_id_keyed(secret, credential),
            None => api_key_id(credential),
        };
        let mut caller = Caller::new(
            identity.principal.unwrap_or_else(|| id.clone()),
            CredentialId::new(CredentialKind::ApiKey, id)?,
            Scopes::default(),
            None,
        )?;

        let q = &self.repos.queries;
        // A recognized credential with no access stays recognized, preventing
        // fallback to another chain member. Resource queries run in order and
        // stop on the first denial or error.
        let targets: [(&str, &[&str]); 4] = [
            (&q.api_key_tenant, &[&key, &resource.tenant]),
            (
                &q.api_key_organization,
                &[&key, &resource.tenant, &resource.org],
            ),
            (
                &q.api_key_workspace,
                &[&key, &resource.tenant, &resource.org, &resource.ws],
            ),
            (
                &q.api_key_environment,
                &[
                    &key,
                    &resource.tenant,
                    &resource.org,
                    &resource.ws,
                    &resource.env,
                ],
            ),
        ];
        for (sql, params) in targets {
            if !self.exists(sql, params).await? {
                return Ok(Some(caller));
            }
        }

        let mut granted = Vec::new();
        for scope in [
            Scope::Read,
            Scope::Write,
            Scope::List,
            Scope::Delete,
            Scope::Publish,
        ] {
            if wanted.contains(scope)
                && self
                    .exists(&q.api_key_scope, &[&key, scope.as_str()])
                    .await?
            {
                granted.push(scope);
            }
        }
        // Construct again through the identity boundary; Caller has no mutable
        // grants exposed outside core.
        caller = Caller::new(
            caller.principal().to_owned(),
            caller.credential().clone(),
            granted.into_iter().collect(),
            None,
        )?;
        Ok(Some(caller))
    }
}
