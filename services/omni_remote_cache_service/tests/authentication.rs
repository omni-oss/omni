use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum_test::{TestRequest, TestServer};
use http::StatusCode;
use omni_remote_cache_core::hooks::HookOutcome;
use omni_remote_cache_core::{
    config::Configuration,
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    hooks::{
        ArtifactHooks, DynArtifactHooks, HookContext, HookDecision, HookError,
    },
    providers::{ConfigBasedDependencyProvider, DependencyProvider},
    security::{
        AuthError, Authenticator, AuthenticatorChain, Caller, CredentialId,
        CredentialKind, DynSecurityService, Resource, Scope, Scopes,
    },
    services::DynValidationService,
    state::{ServiceInfo, ServiceState},
};
use omni_remote_cache_storage_memory::InMemoryBackend;

#[derive(Clone, Copy)]
enum Mode {
    Allowed,
    Denied,
    Unknown,
    Offline,
}

struct Identity {
    mode: Mode,
    calls: Arc<Mutex<Vec<(Resource, Scopes)>>>,
}

#[async_trait]
impl Authenticator for Identity {
    async fn authenticate(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        assert_eq!(credential, "oidc-token");
        self.calls.lock().unwrap().push((resource.clone(), wanted));
        match self.mode {
            Mode::Unknown => Ok(None),
            Mode::Offline => {
                Err(AuthError::Backend(eyre::eyre!("private-database-error")))
            }
            Mode::Allowed | Mode::Denied => Caller::new(
                "external-principal".into(),
                CredentialId::new(
                    CredentialKind::Custom("oidc".into()),
                    "opaque-subject".into(),
                )?,
                if matches!(self.mode, Mode::Allowed) {
                    wanted
                } else {
                    Scopes::default()
                },
                Some("pr-42".into()),
            )
            .map(Some),
        }
    }
}

#[derive(Default)]
struct Hooks(Mutex<Vec<HookContext>>);

#[async_trait]
impl ArtifactHooks for Hooks {
    async fn before(
        &self,
        ctx: &HookContext,
    ) -> Result<HookDecision, HookError> {
        self.0.lock().unwrap().push(ctx.clone());
        Ok(HookDecision::Allow)
    }

    async fn after(
        &self,
        _: &HookContext,
        _: &HookOutcome,
    ) -> Result<(), HookError> {
        Ok(())
    }
}

struct Provider {
    inner: ConfigBasedDependencyProvider,
    auth: Arc<dyn Authenticator>,
    hooks: Arc<Hooks>,
}

impl DependencyProvider for Provider {
    fn authenticator(&self) -> Arc<dyn Authenticator> {
        self.auth.clone()
    }
    fn workspace_repository(&self) -> DynWorkspaceRepository {
        self.inner.workspace_repository()
    }
    fn organization_repository(&self) -> DynOrganizationRepository {
        self.inner.organization_repository()
    }
    fn environment_repository(&self) -> DynEnvironmentRepository {
        self.inner.environment_repository()
    }
    fn tenant_repository(&self) -> DynTenantRepository {
        self.inner.tenant_repository()
    }
    fn validation_service(&self) -> DynValidationService {
        self.inner.validation_service()
    }
    fn security_service(&self) -> DynSecurityService {
        panic!("routes must not use API-key security")
    }
    fn api_key_id(&self, _: &str) -> String {
        panic!("hooks must use the authenticated credential ID")
    }
    fn hooks(&self) -> DynArtifactHooks {
        self.hooks.clone()
    }
}

type Calls = Arc<Mutex<Vec<(Resource, Scopes)>>>;

fn server_for(mode: Mode) -> (TestServer, Calls, Arc<Hooks>) {
    let config = Configuration::from_inline(
        serde_json::json!({
            "tenants": { "t": { "organizations": { "o": { "workspaces": {
                "w": { "environments": { "e": {} } }
            } } } } },
            "security": { "api_keys": {} }
        })
        .to_string(),
    )
    .unwrap();
    let calls = Calls::default();
    let hooks = Arc::new(Hooks::default());
    let provider = Arc::new(Provider {
        inner: ConfigBasedDependencyProvider::new(
            Arc::new(config.to_in_memory_database()),
            Arc::new(config.security.api_keys),
        ),
        auth: Arc::new(AuthenticatorChain::new(vec![Arc::new(Identity {
            mode,
            calls: calls.clone(),
        })])),
        hooks: hooks.clone(),
    });
    let state = ServiceState::new(
        Arc::new(InMemoryBackend::new()),
        Arc::new(ServiceInfo::new(
            "test",
            "0",
            "",
            false,
            Some("/api".into()),
        )),
        provider,
    );
    let router = omni_remote_cache_service::routes::root::build_router(
        &omni_remote_cache_core::routes::root::RouterConfig::new(
            Some("/api".into()),
            false,
        ),
    )
    .with_state(state);
    (TestServer::new(router), calls, hooks)
}

fn request(server: &TestServer, index: usize) -> TestRequest {
    let one = "/api/v1/artifacts/abc?org=o&ws=w&env=e";
    let all = "/api/v1/artifacts?org=o&ws=w&env=e";
    match index {
        0 => server.get(all),
        1 => server.method(http::Method::HEAD, all),
        2 => server.get(one),
        3 => server.method(http::Method::HEAD, one),
        4 => server.put(one).bytes("body".into()),
        _ => server.delete(one),
    }
}

#[tokio::test]
async fn every_route_accepts_a_non_api_key_caller_and_uses_its_id_for_hooks() {
    let (server, calls, hooks) = server_for(Mode::Allowed);
    for (index, expected) in
        [200, 204, 404, 404, 204, 204].into_iter().enumerate()
    {
        request(&server, index)
            .add_header("Authorization", "Bearer oidc-token")
            .add_header("X-OMNI-TENANT", "t")
            .await
            .assert_status(StatusCode::from_u16(expected).unwrap());
    }
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 6);
    for ((resource, wanted), scope) in calls.iter().zip([
        Scope::List,
        Scope::List,
        Scope::Read,
        Scope::Read,
        Scope::Write,
        Scope::Delete,
    ]) {
        assert_eq!(
            resource,
            &Resource {
                tenant: "t".into(),
                org: "o".into(),
                ws: "w".into(),
                env: "e".into()
            }
        );
        assert_eq!(*wanted, scope.into());
    }
    let hooks = hooks.0.lock().unwrap();
    assert_eq!(hooks.len(), 6);
    assert!(hooks.iter().all(|ctx| ctx.api_key_id == "opaque-subject"));
}

#[tokio::test]
async fn every_route_distinguishes_unknown_denied_and_unavailable_without_running_hooks()
 {
    for (mode, status) in [
        (Mode::Unknown, StatusCode::UNAUTHORIZED),
        (Mode::Denied, StatusCode::FORBIDDEN),
        (Mode::Offline, StatusCode::SERVICE_UNAVAILABLE),
    ] {
        let (server, calls, hooks) = server_for(mode);
        for index in 0..6 {
            let response = request(&server, index)
                .add_header("Authorization", "Bearer oidc-token")
                .add_header("X-OMNI-TENANT", "t")
                .await;
            response.assert_status(status);
            assert!(!response.text().contains("private-database-error"));
            assert!(!response.text().contains("oidc-token"));
            if status == StatusCode::UNAUTHORIZED {
                assert_eq!(response.header("www-authenticate"), "Bearer");
            }
        }
        assert_eq!(calls.lock().unwrap().len(), 6);
        assert!(hooks.0.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn legacy_headers_are_rejected_and_authentication_precedes_ownership() {
    let (server, calls, hooks) = server_for(Mode::Allowed);
    for index in 0..6 {
        request(&server, index)
            .add_header("X-API-KEY", "oidc-token")
            .add_header("X-OMNI-TENANT", "t")
            .await
            .assert_status_unauthorized();
    }
    assert!(calls.lock().unwrap().is_empty());

    // An unknown resource is reported only to an authenticated caller: the
    // authenticator runs first, so anonymous callers cannot probe for it.
    let nope = "/api/v1/artifacts/abc?org=nope&ws=w&env=e";
    for mode in [Mode::Unknown, Mode::Denied] {
        let (server, _, _) = server_for(mode);
        let status = server
            .get(nope)
            .add_header("Authorization", "Bearer oidc-token")
            .add_header("X-OMNI-TENANT", "t")
            .await
            .status_code();
        assert!(status == 401 || status == 403, "{status}");
    }
    server
        .get(nope)
        .add_header("Authorization", "Bearer oidc-token")
        .add_header("X-OMNI-TENANT", "t")
        .await
        .assert_status_bad_request();
    assert!(hooks.0.lock().unwrap().is_empty());
}
