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
        HookError, HookOutcome,
    },
    providers::DependencyProvider,
    routes::root::RouterConfig,
    security::DynSecurityService,
    services::DynValidationService,
    state::build_state,
};

struct WithHooks {
    inner: Arc<dyn DependencyProvider>,
    hooks: DynArtifactHooks,
}

impl DependencyProvider for WithHooks {
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
        self.inner.security_service()
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
}

#[async_trait]
impl ArtifactHooks for Recorder {
    async fn before(
        &self,
        ctx: &HookContext,
    ) -> Result<HookDecision, HookError> {
        self.before.fetch_add(1, Ordering::SeqCst);
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
        _: &HookOutcome,
    ) -> Result<(), HookError> {
        self.after.fetch_add(1, Ordering::SeqCst);
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
            api_keys: unordered_map!("k".to_string() => ApiKeyConfiguration {
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
    let router = omni_remote_cache_service::routes::root::build_router(
        &RouterConfig::from(serve_args.routes.clone().unwrap()),
    )
    .with_state(state);
    TestServer::new(router)
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
        r.add_header("X-API-KEY", "k")
            .add_header("X-OMNI-TENANT", "t")
    };

    for r in [
        req(s.get(ONE)),
        req(s.get(LIST)),
        req(s.method(http::Method::HEAD, ONE)),
        req(s.method(http::Method::HEAD, LIST)),
        req(s.put(ONE)).bytes("x".into()),
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
        .add_header("X-API-KEY", "k")
        .add_header("X-OMNI-TENANT", "t")
        .bytes("x".into())
        .await;
    res.assert_status(http::StatusCode::NO_CONTENT);
    assert_eq!(hooks.before.load(Ordering::SeqCst), 1);
    assert_eq!(hooks.after.load(Ordering::SeqCst), 1);
}
