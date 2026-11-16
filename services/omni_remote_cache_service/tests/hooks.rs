use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use axum_test::TestServer;
use maps::unordered_map;
use omni_remote_cache_service::{
    args::{BackendType, ConfigType, RouterArgs, ServeArgs},
    config::{
        All, AllOrSpecificConfiguration, ApiKeyConfiguration, Configuration,
        EnvironmentConfiguration, OrganizationConfiguration,
        SecurityConfiguration, TenantConfiguration, WorkspaceConfiguration,
    },
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    hooks::{
        ArtifactHooks, ArtifactOp, DynArtifactHooks, HookContext, HookDecision,
        HookError, HookOutcome, OutcomeKind,
    },
    providers::DependencyProvider,
    routes::root::RouterConfig,
    security::Authenticator,
    services::DynValidationService,
    state::build_state,
};
use sha2::{Digest as _, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct WithHooks {
    inner: Arc<dyn DependencyProvider>,
    hooks: DynArtifactHooks,
}

impl DependencyProvider for WithHooks {
    fn authenticator(&self) -> Arc<dyn Authenticator> {
        self.inner.authenticator()
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
    fn hooks(&self) -> DynArtifactHooks {
        self.hooks.clone()
    }
}

#[derive(Default)]
struct Recorder {
    veto_ops: Vec<ArtifactOp>,
    before: AtomicUsize,
    after: AtomicUsize,
    key_ids: std::sync::Mutex<Vec<String>>,
    outcomes: std::sync::Mutex<Vec<OutcomeKind>>,
}

#[async_trait]
impl ArtifactHooks for Recorder {
    async fn before(
        &self,
        ctx: &HookContext,
    ) -> Result<HookDecision, HookError> {
        self.before.fetch_add(1, Ordering::SeqCst);
        self.key_ids.lock().unwrap().push(ctx.api_key_id.clone());
        Ok(if self.veto_ops.contains(&ctx.op) {
            HookDecision::Veto {
                reason: "blocked by policy".into(),
            }
        } else {
            HookDecision::Allow
        })
    }

    async fn after(
        &self,
        _: &HookContext,
        outcome: &HookOutcome,
    ) -> Result<(), HookError> {
        self.after.fetch_add(1, Ordering::SeqCst);
        self.outcomes.lock().unwrap().push(outcome.kind);
        Ok(())
    }
}

fn config() -> Configuration {
    Configuration {
        tenants: unordered_map!("t".to_string() => TenantConfiguration {
            description: None,
            display_name: None,
            organizations: unordered_map!("o".to_string() => OrganizationConfiguration {
                description: None,
                display_name: None,
                workspaces: unordered_map!("w".to_string() => WorkspaceConfiguration {
                    description: None,
                    display_name: None,
                    environments: unordered_map!("e".to_string() => EnvironmentConfiguration {
                        description: None,
                        display_name: None,
                    }),
                }),
            }),
        }),
        security: SecurityConfiguration {
            api_key_id_secret: None,
            api_keys: unordered_map!("k".to_string() => ApiKeyConfiguration {
                principal: None,
                description: None,
                enabled: true,
                expires_at: None,
                workspaces: AllOrSpecificConfiguration::new_all(All::new_all()),
                environments: AllOrSpecificConfiguration::new_all(All::new_all()),
                organizations: AllOrSpecificConfiguration::new_all(All::new_all()),
                scopes: AllOrSpecificConfiguration::new_all(All::new_all()),
                tenants: AllOrSpecificConfiguration::new_all(All::new_all()),
            }),
        },
    }
}

async fn server(hooks: Arc<Recorder>) -> TestServer {
    TestServer::new(router(hooks, None).await)
}

async fn router(
    hooks: Arc<Recorder>,
    max_artifact_size: Option<u64>,
) -> axum::Router {
    let serve_args = ServeArgs::new(
        "".to_string(),
        Some(serde_json::to_string(&config()).unwrap()),
        Some(ConfigType::Inline),
        false,
        None,
        None,
        Some(100),
        BackendType::InMemory,
        Some(RouterArgs::new(Some("/api".to_string()), false)),
        None,
    );
    let mut state = build_state(&serve_args).await.unwrap();
    state.provider = Arc::new(WithHooks {
        inner: state.provider.clone(),
        hooks,
    });
    if let Some(limit) = max_artifact_size {
        state.max_artifact_size = limit;
    }
    omni_remote_cache_service::routes::root::build_router(&RouterConfig::from(
        serve_args.routes.clone().unwrap(),
    ))
    .with_state(state)
}

const ONE: &str = "/api/v1/artifacts/abc?org=o&ws=w&env=e";
const LIST: &str = "/api/v1/artifacts?org=o&ws=w&env=e";

#[tokio::test]
async fn every_operation_is_vetoable_and_a_veto_skips_after() {
    let hooks = Arc::new(Recorder {
        veto_ops: vec![
            ArtifactOp::Get,
            ArtifactOp::GetMany,
            ArtifactOp::Head,
            ArtifactOp::HeadMany,
            ArtifactOp::Put,
            ArtifactOp::Delete,
        ],
        ..Default::default()
    });
    let s = server(hooks.clone()).await;
    let req = |r: axum_test::TestRequest| {
        r.add_header("Authorization", "Bearer k")
            .add_header("X-OMNI-TENANT", "t")
    };

    for r in [
        req(s.get(ONE)),
        req(s.get(LIST)),
        req(s.method(http::Method::HEAD, ONE)),
        req(s.method(http::Method::HEAD, LIST)),
        req(s.put(ONE))
            .add_header("X-Omni-Content-Sha256", sha256_hex(b"x"))
            .bytes("x".into()),
        req(s.delete(ONE)),
    ] {
        let res = r.await;
        res.assert_status(http::StatusCode::FORBIDDEN);
        if !res.text().is_empty() {
            assert!(res.text().contains("blocked by policy"));
        }
    }
    assert_eq!(hooks.before.load(Ordering::SeqCst), 6);
    assert_eq!(hooks.after.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn allowed_operation_runs_after() {
    let hooks = Arc::new(Recorder::default());
    let s = server(hooks.clone()).await;
    let res = s
        .put(ONE)
        .add_header("Authorization", "Bearer k")
        .add_header("X-OMNI-TENANT", "t")
        .add_header("X-Omni-Content-Sha256", sha256_hex(b"x"))
        .bytes("x".into())
        .await;
    res.assert_status(http::StatusCode::NO_CONTENT);
    assert_eq!(hooks.before.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.after.load(Ordering::SeqCst), 1);

    // hooks see an opaque id, never the key itself
    let ids = hooks.key_ids.lock().unwrap();
    assert_eq!(ids[0], omni_remote_cache_service::hooks::api_key_id("k"));
    assert_ne!(ids[0], "k");
    assert_eq!(ids[0].len(), 16);
}

#[tokio::test]
async fn a_malformed_digest_never_reaches_the_hooks() {
    let hooks = Arc::new(Recorder::default());
    let s = server(hooks.clone()).await;
    let bad = "/api/v1/artifacts/not.a.digest?org=o&ws=w&env=e";
    let req = |r: axum_test::TestRequest| {
        r.add_header("Authorization", "Bearer k")
            .add_header("X-OMNI-TENANT", "t")
    };
    for r in [
        req(s.get(bad)),
        req(s.method(http::Method::HEAD, bad)),
        req(s.put(bad))
            .add_header("X-Omni-Content-Sha256", sha256_hex(b"x"))
            .bytes("x".into()),
        req(s.delete(bad)),
    ] {
        r.await.assert_status(http::StatusCode::BAD_REQUEST);
    }
    assert_eq!(hooks.before.load(Ordering::SeqCst), 0);
    assert_eq!(hooks.after.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_conflicting_put_reports_already_exists_to_the_hooks() {
    let hooks = Arc::new(Recorder::default());
    let s = server(hooks.clone()).await;
    let put = || async {
        s.put(ONE)
            .add_header("Authorization", "Bearer k")
            .add_header("X-OMNI-TENANT", "t")
            .add_header("X-Omni-Content-Sha256", sha256_hex(b"x"))
            .bytes("x".into())
            .await
            .status_code()
    };
    assert_eq!(put().await, http::StatusCode::NO_CONTENT);
    assert_eq!(put().await, http::StatusCode::CONFLICT);
    let outcomes = hooks.outcomes.lock().unwrap();
    assert_eq!(
        *outcomes,
        [
            omni_remote_cache_service::hooks::OutcomeKind::Stored,
            omni_remote_cache_service::hooks::OutcomeKind::AlreadyExists,
        ]
    );
}

#[tokio::test]
async fn a_declared_oversized_upload_never_reaches_the_hooks() {
    use tower::ServiceExt as _;
    let hooks = Arc::new(Recorder::default());
    let router = router(hooks.clone(), Some(4)).await;
    // The in-process test client sends no Content-Length of its own, so set it:
    // this is the header path that is refused before the body is read.
    let request = http::Request::builder()
        .method("PUT")
        .uri(ONE)
        .header("Authorization", "Bearer k")
        .header("X-OMNI-TENANT", "t")
        .header("X-Omni-Content-Sha256", sha256_hex(b"12345"))
        .header(http::header::CONTENT_LENGTH, "5")
        .body(axum::body::Body::from("12345"))
        .unwrap();
    let status = router.oneshot(request).await.unwrap().status();
    assert_eq!(status, http::StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(hooks.before.load(Ordering::SeqCst), 0);
    assert_eq!(hooks.after.load(Ordering::SeqCst), 0);
}

async fn stream_put(
    router: &axum::Router,
    chunks: Vec<Result<&'static str, &'static str>>,
) -> http::StatusCode {
    use tower::ServiceExt as _;
    let digest = sha256_hex(
        &chunks
            .iter()
            .filter_map(|c| c.ok())
            .flat_map(|c| c.bytes())
            .collect::<Vec<u8>>(),
    );
    // No Content-Length: only the stream bound, or a stream error, can stop it.
    let body = axum::body::Body::from_stream(tokio_stream::iter(
        chunks
            .into_iter()
            .map(|c| c.map(bytes::Bytes::from).map_err(std::io::Error::other)),
    ));
    let request = http::Request::builder()
        .method("PUT")
        .uri(ONE)
        .header("Authorization", "Bearer k")
        .header("X-OMNI-TENANT", "t")
        .header("X-Omni-Content-Sha256", digest)
        .body(body)
        .unwrap();
    router.clone().oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn a_streamed_oversized_upload_is_413_and_reported_as_failed() {
    let hooks = Arc::new(Recorder::default());
    let router = router(hooks.clone(), Some(4)).await;
    assert_eq!(
        stream_put(&router, vec![Ok("123"), Ok("45")]).await,
        http::StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(hooks.before.load(Ordering::SeqCst), 1);
    assert_eq!(
        hooks.after.load(Ordering::SeqCst),
        1,
        "before needs an after"
    );
    assert_eq!(
        *hooks.outcomes.lock().unwrap(),
        [omni_remote_cache_service::hooks::OutcomeKind::Failed]
    );
}

#[tokio::test]
async fn a_body_error_under_the_limit_is_not_mistaken_for_413() {
    let hooks = Arc::new(Recorder::default());
    let router = router(hooks.clone(), Some(1024)).await;
    assert_eq!(
        stream_put(&router, vec![Ok("12"), Err("connection reset")]).await,
        http::StatusCode::INTERNAL_SERVER_ERROR
    );
}
