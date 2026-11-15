use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use omni_remote_cache_core::{
    caching::{
        CacheConfiguration, CachingConfiguration, CachingProvider, CachingState,
    },
    hooks::api_key_id_keyed,
    providers::DependencyProvider,
    security::{Resource, Scope, Scopes},
};
use omni_remote_cache_repository_query::{
    ApiKeyHashConfiguration, ApiKeyIdentity, QueriesConfiguration,
    QueryDependencyProvider, QueryError, QueryExecutor,
};

#[tokio::test]
async fn rebuilt_provider_wrappers_share_cached_callers() {
    let mut steps = vec![identity(Some("query-owner"))];
    steps.extend(resource_checks());
    steps.push(check("api_key_scope", &["read:artifacts"], true));
    let (provider, script) = provider(steps);
    let inner = Arc::new(provider);
    let config = CachingConfiguration {
        repositories: None,
        security: Some(CacheConfiguration {
            ttl_secs: 60,
            max_entries: 10,
            negative_ttl_secs: None,
        }),
    };
    let state = Arc::new(CachingState::new(&config));
    for _ in 0..2 {
        let wrapped = CachingProvider::with_state(inner.clone(), state.clone());
        let caller = wrapped
            .authenticator()
            .authenticate("credential", &target(), Scope::Read.into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(caller.principal(), "query-owner");
        assert!(caller.can(Scope::Read));
    }
    script.done();
}
enum Step {
    Identity(Result<Option<ApiKeyIdentity>, QueryError>),
    Check(&'static str, Vec<String>, Result<bool, QueryError>),
}

struct Script {
    steps: Mutex<VecDeque<Step>>,
}

impl Script {
    fn pop(&self) -> Step {
        self.steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected query")
    }

    fn done(&self) {
        assert!(
            self.steps.lock().unwrap().is_empty(),
            "queries were skipped"
        );
    }
}

#[async_trait]
impl QueryExecutor for Script {
    async fn api_key_identity(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Option<ApiKeyIdentity>, QueryError> {
        assert_eq!(sql, "api_key_valid");
        assert_eq!(
            params,
            [ApiKeyHashConfiguration::default().apply("credential")]
        );
        let Step::Identity(answer) = self.pop() else {
            panic!("expected identity lookup first")
        };
        answer
    }

    async fn exists(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<bool, QueryError> {
        let Step::Check(expected_sql, expected_params, answer) = self.pop()
        else {
            panic!("unexpected identity lookup")
        };
        assert_eq!(sql, expected_sql);
        assert_eq!(params, expected_params);
        assert!(
            !params.contains(&"credential"),
            "raw key must never be bound"
        );
        answer
    }

    async fn veto_reason(
        &self,
        _: &str,
        _: &[&str],
    ) -> Result<Option<String>, QueryError> {
        panic!("authentication must not run hooks")
    }

    async fn execute(&self, _: &str, _: &[&str]) -> Result<(), QueryError> {
        panic!("authentication must not write")
    }
}

fn queries() -> QueriesConfiguration {
    QueriesConfiguration {
        tenant_exists: "tenant_exists".into(),
        organization_exists: "organization_exists".into(),
        organization_belongs_to_tenant: "organization_belongs_to_tenant".into(),
        workspace_exists: "workspace_exists".into(),
        workspace_belongs_to_organization: "workspace_belongs_to_organization"
            .into(),
        environment_exists: "environment_exists".into(),
        environment_belongs_to_workspace: "environment_belongs_to_workspace"
            .into(),
        api_key_valid: "api_key_valid".into(),
        api_key_tenant: "api_key_tenant".into(),
        api_key_organization: "api_key_organization".into(),
        api_key_workspace: "api_key_workspace".into(),
        api_key_environment: "api_key_environment".into(),
        api_key_scope: "api_key_scope".into(),
    }
}

fn target() -> Resource {
    Resource {
        tenant: "t".into(),
        org: "o".into(),
        ws: "w".into(),
        env: "e".into(),
    }
}

fn identity(principal: Option<&str>) -> Step {
    Step::Identity(Ok(Some(ApiKeyIdentity {
        found: true,
        principal: principal.map(str::to_owned),
    })))
}

fn check(query: &'static str, params: &[&str], answer: bool) -> Step {
    let key = ApiKeyHashConfiguration::default().apply("credential");
    let params = std::iter::once(key)
        .chain(params.iter().map(|s| s.to_string()))
        .collect();
    Step::Check(query, params, Ok(answer))
}

fn resource_checks() -> Vec<Step> {
    vec![
        check("api_key_tenant", &["t"], true),
        check("api_key_organization", &["t", "o"], true),
        check("api_key_workspace", &["t", "o", "w"], true),
        check("api_key_environment", &["t", "o", "w", "e"], true),
    ]
}

fn provider(steps: Vec<Step>) -> (QueryDependencyProvider, Arc<Script>) {
    let script = Arc::new(Script {
        steps: Mutex::new(steps.into()),
    });
    let provider = QueryDependencyProvider::new(
        script.clone(),
        queries(),
        ApiKeyHashConfiguration::default(),
    )
    .unwrap()
    .with_api_key_id_secret(Some("secret".into()));
    (provider, script)
}

#[tokio::test]
async fn legacy_identity_defaults_to_the_keyed_id_and_queries_only_wanted_scope()
 {
    let mut steps = vec![identity(None)];
    steps.extend(resource_checks());
    steps.push(check("api_key_scope", &["read:artifacts"], true));
    let (provider, script) = provider(steps);
    let caller = provider
        .authenticator()
        .authenticate("credential", &target(), Scope::Read.into())
        .await
        .unwrap()
        .unwrap();
    let id = api_key_id_keyed("secret", "credential");
    assert_eq!(caller.principal(), id);
    assert_eq!(caller.credential().id(), provider.api_key_id("credential"));
    assert_eq!(caller.scope_pin(), None);
    assert!(caller.can(Scope::Read));
    assert!(!caller.can(Scope::Write));
    assert!(!caller.can(Scope::Publish));
    script.done();
}

#[tokio::test]
async fn explicit_principal_and_each_wanted_scope_are_resolved_independently() {
    let mut steps = vec![identity(Some("stable-owner"))];
    steps.extend(resource_checks());
    steps.extend([
        check("api_key_scope", &["read:artifacts"], true),
        check("api_key_scope", &["write:artifacts"], false),
        check("api_key_scope", &["publish:artifacts"], true),
    ]);
    let (provider, script) = provider(steps);
    let caller = provider
        .authenticator()
        .authenticate(
            "credential",
            &target(),
            [Scope::Publish, Scope::Write, Scope::Read].into(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(caller.principal(), "stable-owner");
    assert_eq!(caller.scopes(), [Scope::Read, Scope::Publish].into());
    script.done();
}

#[tokio::test]
async fn no_wanted_scopes_runs_no_scope_queries_but_still_checks_the_resource()
{
    let mut steps = vec![identity(None)];
    steps.extend(resource_checks());
    let (provider, script) = provider(steps);
    let caller = provider
        .authenticator()
        .authenticate("credential", &target(), Scopes::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(caller.scopes(), Scopes::default());
    script.done();
}

#[tokio::test]
async fn each_resource_denial_remains_recognized_and_short_circuits() {
    for denied_at in 0..4 {
        let mut checks = resource_checks();
        checks.truncate(denied_at + 1);
        let Step::Check(_, _, answer) = checks.last_mut().unwrap() else {
            unreachable!()
        };
        *answer = Ok(false);
        let mut steps = vec![identity(Some("owner"))];
        steps.extend(checks);
        let (provider, script) = provider(steps);
        let caller = provider
            .authenticator()
            .authenticate("credential", &target(), Scope::Read.into())
            .await
            .unwrap()
            .expect("resource denial must not fall through");
        assert_eq!(caller.scopes(), Scopes::default());
        script.done();
    }
}

#[tokio::test]
async fn absent_and_invalid_identity_short_circuit_before_resource_queries() {
    for answer in [
        None,
        Some(ApiKeyIdentity {
            found: false,
            principal: None,
        }),
        Some(ApiKeyIdentity {
            found: false,
            principal: Some("".into()),
        }),
    ] {
        let (provider, script) = provider(vec![Step::Identity(Ok(answer))]);
        assert!(
            provider
                .authenticator()
                .authenticate("credential", &target(), Scope::Read.into())
                .await
                .unwrap()
                .is_none()
        );
        script.done();
    }
}

#[tokio::test]
async fn errors_at_each_query_boundary_fail_closed() {
    // Validity, four resource checks, and the requested scope.
    for error_at in 0..6 {
        let mut steps = vec![identity(None)];
        steps.extend(resource_checks());
        steps.push(check("api_key_scope", &["read:artifacts"], true));
        steps.truncate(error_at + 1);
        let error = QueryError::Driver("private backend detail".into());
        match steps.last_mut().unwrap() {
            Step::Identity(answer) => *answer = Err(error),
            Step::Check(_, _, answer) => *answer = Err(error),
        }
        let (provider, script) = provider(steps);
        let error = provider
            .authenticator()
            .authenticate("credential", &target(), Scope::Read.into())
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "authentication backend unavailable");
        script.done();
    }
}

#[tokio::test]
async fn malformed_principal_is_an_error_not_an_identity_fallback() {
    for principal in ["".into(), "bad\nname".into(), "x".repeat(257)] {
        let (provider, script) = provider(vec![identity(Some(&principal))]);
        assert!(
            provider
                .authenticator()
                .authenticate("credential", &target(), Scope::Read.into())
                .await
                .is_err()
        );
        script.done();
    }
}
