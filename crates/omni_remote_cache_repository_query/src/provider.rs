use std::sync::Arc;

use omni_remote_cache_core::{
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    hooks::{Background, DynArtifactHooks, HookSettings, NoopHooks},
    providers::DependencyProvider,
    security::DynSecurityService,
    services::{DefaultValidationService, DynValidationService},
};

use crate::{
    ApiKeyHashConfiguration, QueriesConfiguration, QueryExecutor,
    QueryRepositories, QuerySecurityService,
};

/// A `DependencyProvider` whose data comes from operator queries run by a
/// `QueryExecutor`. Hooks and the background runner are optional.
pub struct QueryDependencyProvider {
    repos: QueryRepositories,
    hash: Arc<ApiKeyHashConfiguration>,
    hooks: DynArtifactHooks,
    hook_settings: HookSettings,
    background: Option<Arc<dyn Background>>,
}

impl QueryDependencyProvider {
    pub fn new(
        executor: Arc<dyn QueryExecutor>,
        queries: QueriesConfiguration,
        hash: ApiKeyHashConfiguration,
    ) -> eyre::Result<Self> {
        queries.validate().map_err(|e| eyre::eyre!(e))?;
        hash.validate()?;
        Ok(Self {
            repos: QueryRepositories::new(executor, Arc::new(queries)),
            hash: Arc::new(hash),
            hooks: Arc::new(NoopHooks),
            hook_settings: HookSettings::default(),
            background: None,
        })
    }

    pub fn with_hooks(
        mut self,
        hooks: DynArtifactHooks,
        settings: HookSettings,
    ) -> Self {
        self.hooks = hooks;
        self.hook_settings = settings;
        self
    }

    pub fn with_background(mut self, background: Arc<dyn Background>) -> Self {
        self.background = Some(background);
        self
    }
}

impl DependencyProvider for QueryDependencyProvider {
    fn workspace_repository(&self) -> DynWorkspaceRepository {
        Box::new(self.repos.clone())
    }

    fn organization_repository(&self) -> DynOrganizationRepository {
        Box::new(self.repos.clone())
    }

    fn environment_repository(&self) -> DynEnvironmentRepository {
        Box::new(self.repos.clone())
    }

    fn tenant_repository(&self) -> DynTenantRepository {
        Box::new(self.repos.clone())
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
        Box::new(QuerySecurityService::new(
            self.repos.clone(),
            self.hash.clone(),
        ))
    }

    fn hooks(&self) -> DynArtifactHooks {
        self.hooks.clone()
    }

    fn hook_settings(&self) -> HookSettings {
        self.hook_settings
    }

    fn background(&self) -> Option<Arc<dyn Background>> {
        self.background.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::QueryError;

    #[derive(Default)]
    struct Recording {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        answer: bool,
    }

    #[async_trait]
    impl QueryExecutor for Recording {
        async fn exists(
            &self,
            sql: &str,
            params: &[&str],
        ) -> Result<bool, QueryError> {
            self.calls.lock().unwrap().push((
                sql.to_string(),
                params.iter().map(|p| p.to_string()).collect(),
            ));
            Ok(self.answer)
        }
    }

    fn queries() -> QueriesConfiguration {
        let q = |n: &str| format!("SELECT 1 AS found /* {n} */");
        QueriesConfiguration {
            tenant_exists: q("tenant_exists"),
            organization_exists: q("organization_exists"),
            organization_belongs_to_tenant: q("organization_belongs_to_tenant"),
            workspace_exists: q("workspace_exists"),
            workspace_belongs_to_organization: q(
                "workspace_belongs_to_organization",
            ),
            environment_exists: q("environment_exists"),
            environment_belongs_to_workspace: q(
                "environment_belongs_to_workspace",
            ),
            api_key_valid: q("api_key_valid"),
            api_key_tenant: q("api_key_tenant"),
            api_key_organization: q("api_key_organization"),
            api_key_workspace: q("api_key_workspace"),
            api_key_environment: q("api_key_environment"),
            api_key_scope: q("api_key_scope"),
        }
    }

    #[tokio::test]
    async fn request_values_only_reach_the_query_as_bound_params() {
        let exec = Arc::new(Recording {
            answer: true,
            ..Default::default()
        });
        let evil = "x'; DROP TABLE t; --";
        let p = QueryDependencyProvider::new(
            exec.clone(),
            queries(),
            ApiKeyHashConfiguration::default(),
        )
        .unwrap();

        p.validation_service()
            .validate_ownership(evil, evil, evil, evil)
            .await
            .unwrap();
        p.security_service()
            .can_access(
                "secret-key",
                evil,
                evil,
                evil,
                evil,
                &["read:artifacts"],
            )
            .await
            .unwrap();

        let calls = exec.calls.lock().unwrap();
        assert!(calls.len() > 8);
        let hashed = ApiKeyHashConfiguration::default().apply("secret-key");
        for (sql, params) in calls.iter() {
            assert!(sql.starts_with("SELECT 1 AS found"), "{sql}");
            assert!(!sql.contains("DROP") && !sql.contains("secret-key"));
            assert!(!params.iter().any(|p| p == "secret-key"));
        }
        assert!(calls.iter().any(|(_, p)| p.first() == Some(&hashed)));
        assert!(calls.iter().any(|(s, p)| s.contains("api_key_scope")
            && p == &[hashed.clone(), "read:artifacts".to_string()]));
    }

    #[tokio::test]
    async fn invalid_key_short_circuits_and_false_denies() {
        let exec = Arc::new(Recording::default());
        let p = QueryDependencyProvider::new(
            exec.clone(),
            queries(),
            ApiKeyHashConfiguration::default(),
        )
        .unwrap();
        let ok = p
            .security_service()
            .can_access("k", "t", "o", "w", "e", &["read:artifacts"])
            .await
            .unwrap();
        assert!(!ok);
        assert_eq!(exec.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn rejects_empty_queries_and_hmac_without_secret() {
        let mut q = queries();
        q.api_key_scope = "  ".into();
        assert!(
            QueryDependencyProvider::new(
                Arc::new(Recording::default()),
                q,
                Default::default()
            )
            .is_err()
        );
        let h = ApiKeyHashConfiguration {
            algorithm: crate::HashAlgorithm::HmacSha256,
            ..Default::default()
        };
        assert!(
            QueryDependencyProvider::new(
                Arc::new(Recording::default()),
                queries(),
                h
            )
            .is_err()
        );
    }
}
