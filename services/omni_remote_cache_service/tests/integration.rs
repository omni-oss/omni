use axum_test::{TestResponse, TestServer};
use bytes::Bytes;
use http::StatusCode;
use maps::unordered_map;
use omni_remote_cache_service::{
    args::{BackendType, ConfigType, RouterArgs, ServeArgs},
    config::{
        All, AllOrSpecificConfiguration, ApiKeyConfiguration, Configuration,
        EnvironmentConfiguration, OrganizationConfiguration,
        ScopesConfiguration, SecurityConfiguration, TenantConfiguration,
        WorkspaceConfiguration,
    },
    response::data::Data,
    routes::{root::RouterConfig, v1::artifacts::CacheItem},
    state::build_state,
};
use sets::{UnorderedSet, unordered_set};

fn default_config() -> Configuration {
    Configuration {
        tenants: unordered_map!(
            DEFAULT_TENANT.to_string() => TenantConfiguration {
                description: None,
                display_name: None,
                organizations: unordered_map!(
                    DEFAULT_ORG.to_string() => OrganizationConfiguration {
                        description: None,
                        display_name: None,
                        workspaces: unordered_map!(
                            DEFAULT_WORKSPACE.to_string() => WorkspaceConfiguration {
                                description: None,
                                display_name: None,
                                environments: unordered_map!(
                                    DEFAULT_ENV.to_string() => EnvironmentConfiguration {
                                        description: None,
                                        display_name: None,
                                    }
                                )
                            }
                        )
                    }
                )
            }
        ),
        security: SecurityConfiguration {
            api_key_id_secret: None,
            api_keys: unordered_map!(
                "test-api-key".to_string() => ApiKeyConfiguration {
                    principal: None,
                    description: None,
                    enabled: true,
                    expires_at: None,
                    workspaces: AllOrSpecificConfiguration::new_all(All::new_all()),
                    environments: AllOrSpecificConfiguration::new_all(All::new_all()),
                    organizations: AllOrSpecificConfiguration::new_all(All::new_all()),
                    scopes: AllOrSpecificConfiguration::new_all(All::new_all()),
                    tenants: AllOrSpecificConfiguration::new_all(All::new_all()),
                }
            ),
        },
    }
}

fn default_config_with_scopes(
    scopes: UnorderedSet<ScopesConfiguration>,
) -> Configuration {
    let mut cfg = default_config();

    cfg.security
        .api_keys
        .get_mut(DEFAULT_API_KEY)
        .unwrap()
        .scopes = AllOrSpecificConfiguration::Specific(scopes);

    cfg
}

fn default_body() -> Bytes {
    let config = default_config();
    let vec = serde_json::to_vec(&config)
        .expect("should be able to serialize to json");

    Bytes::from(vec)
}

async fn create_server(cfg: &Configuration) -> TestServer {
    let json_config = serde_json::to_string(cfg)
        .expect("should be able to serialize to json");

    let route_config = RouterArgs::new(Some("/api".to_string()), true);

    let serve_args = ServeArgs::new(
        "".to_string(), // since test server doesn't actually listen, just use an empty string
        Some(json_config),
        Some(ConfigType::Inline),
        false,
        None,
        None,
        Some(100),
        BackendType::InMemory,
        Some(route_config),
        None,
    );
    let state = build_state(&serve_args)
        .await
        .expect("must be able to construct state");
    let router = omni_remote_cache_service::routes::root::build_router(
        &RouterConfig::from(serve_args.routes.clone().unwrap()),
    )
    .with_state(state);

    TestServer::new(router)
}

const DEFAULT_DIGEST: &str =
    "5RkZ7vQ2mXwYpLt3hNcFbA9dGjEsKuVoWiHaBxCqTyMnPrDfUgSe8JzL";

const NEW_DIGEST: &str = "3vQ7mXwYpLt5hNcFbA9dGjEsKuVoWiHaBxCq";

const DEFAULT_ORG: &str = "test-org";
const DEFAULT_TENANT: &str = "test-tenant";
const DEFAULT_WORKSPACE: &str = "test-workspace";
const DEFAULT_ENV: &str = "test-env";
const DEFAULT_API_KEY: &str = "test-api-key";

#[tokio::test]
async fn static_config_secret_is_applied_to_caller_ids() {
    let mut config = default_config();
    config.security.api_key_id_secret = Some("test-id-secret".into());
    let args = ServeArgs::new(
        "".into(),
        Some(serde_json::to_string(&config).unwrap()),
        Some(ConfigType::Inline),
        false,
        None,
        None,
        None,
        BackendType::InMemory,
        Some(RouterArgs::new(Some("/api".into()), false)),
        None,
    );
    let state = build_state(&args).await.unwrap();
    let caller = state
        .provider
        .authenticator()
        .authenticate(
            DEFAULT_API_KEY,
            &omni_remote_cache_core::security::Resource {
                tenant: DEFAULT_TENANT.into(),
                org: DEFAULT_ORG.into(),
                ws: DEFAULT_WORKSPACE.into(),
                env: DEFAULT_ENV.into(),
            },
            omni_remote_cache_core::security::Scope::Read.into(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        caller.credential().id(),
        omni_remote_cache_core::hooks::api_key_id_keyed(
            "test-id-secret",
            DEFAULT_API_KEY
        )
    );
}

#[tokio::test]
async fn interrupted_put_preserves_the_existing_local_artifact() {
    use axum::body::Body;
    use omni_remote_cache_storage_core::{
        Container, RemoteCacheStorageBackend,
    };
    use omni_remote_cache_storage_local::LocalDiskCacheBackend;
    use std::sync::Arc;
    use tower::ServiceExt;

    let args = ServeArgs::new(
        "".to_string(),
        Some(serde_json::to_string(&default_config()).unwrap()),
        Some(ConfigType::Inline),
        false,
        None,
        None,
        None,
        BackendType::InMemory,
        None,
        None,
    );
    let mut state = build_state(&args).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalDiskCacheBackend::new(root.path()));
    state.storage_backend = backend.clone();
    let router = omni_remote_cache_service::routes::root::build_router(
        &RouterConfig::from(RouterArgs::new(Some("/api".to_string()), false)),
    )
    .with_state(state);
    let container = Container::parse(format!(
        "{DEFAULT_TENANT}/{DEFAULT_ORG}/{DEFAULT_WORKSPACE}/{DEFAULT_ENV}"
    ))
    .unwrap();
    backend
        .save_if_absent(
            &container,
            DEFAULT_DIGEST,
            Bytes::from("complete archive"),
        )
        .await
        .unwrap();

    for digest in [DEFAULT_DIGEST, NEW_DIGEST] {
        let stream = tokio_stream::iter([
            Ok(Bytes::from("partial")),
            Err(std::io::Error::other("interrupted request body")),
        ]);
        let request = http::Request::builder()
            .method("PUT")
            .uri(get_path(
                DEFAULT_ORG,
                DEFAULT_WORKSPACE,
                DEFAULT_ENV,
                Some(digest),
            ))
            .header("Authorization", format!("Bearer {DEFAULT_API_KEY}"))
            .header("X-OMNI-TENANT", DEFAULT_TENANT)
            .body(Body::from_stream(stream))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "the authorized PUT must reach storage and report the stream error"
        );
    }
    assert_eq!(
        backend.get(&container, DEFAULT_DIGEST).await.unwrap(),
        Some(Bytes::from("complete archive"))
    );
    assert!(backend.get(&container, NEW_DIGEST).await.unwrap().is_none());
}

fn get_path(
    org: &str,
    workspace: &str,
    env: &str,
    digest: Option<&str>,
) -> String {
    if let Some(digest) = digest {
        format!("/api/v1/artifacts/{digest}?org={org}&ws={workspace}&env={env}",)
    } else {
        format!("/api/v1/artifacts?org={org}&ws={workspace}&env={env}",)
    }
}

async fn get_artifacts(
    server: &TestServer,
    tenant: &str,
    api_key: &str,
    org: &str,
    workspace: &str,
    env: &str,
) -> TestResponse {
    let path = get_path(org, workspace, env, None);
    server
        .get(&path)
        .add_header("Authorization", format!("Bearer {api_key}"))
        .add_header("X-OMNI-TENANT", tenant)
        .await
}

#[allow(clippy::too_many_arguments)]
async fn put_artifact(
    server: &TestServer,
    tenant: &str,
    api_key: &str,
    org: &str,
    workspace: &str,
    env: &str,
    digest: &str,
    body: Bytes,
) -> TestResponse {
    let path = get_path(org, workspace, env, Some(digest));
    server
        .put(&path)
        .add_header("Authorization", format!("Bearer {api_key}"))
        .add_header("Content-Type", "application/octet-stream")
        .add_header("X-OMNI-TENANT", tenant)
        .bytes(body)
        .await
}

async fn get_artifact(
    server: &TestServer,
    tenant: &str,
    api_key: &str,
    org: &str,
    workspace: &str,
    env: &str,
    digest: &str,
) -> TestResponse {
    let path = get_path(org, workspace, env, Some(digest));
    server
        .get(&path)
        .add_header("Authorization", format!("Bearer {api_key}"))
        .add_header("X-OMNI-TENANT", tenant)
        .await
}

async fn head_artifact(
    server: &TestServer,
    tenant: &str,
    api_key: &str,
    org: &str,
    workspace: &str,
    env: &str,
    digest: &str,
) -> TestResponse {
    let path = get_path(org, workspace, env, Some(digest));
    server
        .method(http::Method::HEAD, &path)
        .add_header("Authorization", format!("Bearer {api_key}"))
        .add_header("X-OMNI-TENANT", tenant)
        .await
}

async fn delete_artifact(
    server: &TestServer,
    tenant: &str,
    api_key: &str,
    org: &str,
    workspace: &str,
    env: &str,
    digest: &str,
) -> TestResponse {
    let path = get_path(org, workspace, env, Some(digest));
    server
        .delete(&path)
        .add_header("Authorization", format!("Bearer {api_key}"))
        .add_header("X-OMNI-TENANT", tenant)
        .await
}

#[tokio::test]
async fn test_get_artifact() {
    let cfg = default_config();
    let server = create_server(&cfg).await;
    let body = default_body();

    put_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
        body.clone(),
    )
    .await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_ok();
    get_resp.assert_header("Content-Type", "application/octet-stream");
    assert_eq!(
        *get_resp.as_bytes(),
        body,
        "should be able to get the artifact"
    );
}

#[tokio::test]
async fn test_put_artifact() {
    let cfg = default_config();
    let server = create_server(&cfg).await;
    let body = default_body();

    let put_resp = put_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
        body.clone(),
    )
    .await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    put_resp.assert_status(StatusCode::NO_CONTENT);
    get_resp.assert_status_ok();
    assert_eq!(
        *get_resp.as_bytes(),
        body,
        "should be able to get the artifact"
    );
}

#[tokio::test]
async fn test_delete_artifact() {
    let cfg = default_config();
    let server = create_server(&cfg).await;
    let body = default_body();

    put_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
        body.clone(),
    )
    .await;

    let delete_resp = delete_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    let get_after_delete_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    delete_resp.assert_status(StatusCode::NO_CONTENT);
    get_after_delete_resp.assert_status_not_found();
}

#[tokio::test]
async fn test_get_artifacts() {
    let cfg = default_config();
    let server = create_server(&cfg).await;
    let body = default_body();

    put_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
        body.clone(),
    )
    .await;

    let get_resp = get_artifacts(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
    )
    .await;

    get_resp.assert_status_ok();
    get_resp.assert_header("Content-Type", "application/json");
    let data = Data::new(vec![CacheItem {
        digest: DEFAULT_DIGEST.to_string(),
        size: body.len() as u64,
    }]);
    get_resp.assert_json(&data);
}

#[tokio::test]
async fn test_invalid_api_key() {
    let cfg = default_config();
    let server = create_server(&cfg).await;
    let body = default_body();

    put_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
        body.clone(),
    )
    .await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        "invalid-api-key",
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_unauthorized();
}

#[tokio::test]
async fn test_invalid_tenant() {
    let cfg = default_config();
    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        "invalid-tenant",
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_bad_request();
}

#[tokio::test]
async fn test_invalid_org() {
    let cfg = default_config();
    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        "invalid-org",
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_bad_request();
}

#[tokio::test]
async fn test_invalid_workspace() {
    let cfg = default_config();
    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        "invalid-workspace",
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_bad_request();
}

#[tokio::test]
async fn test_invalid_env() {
    let cfg = default_config();
    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        "invalid-env",
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_bad_request();
}

#[tokio::test]
async fn test_invalid_digest() {
    let cfg = default_config();
    let server = create_server(&cfg).await;
    let body = default_body();
    let too_long = "1".repeat(65);

    for digest in [
        "invalid-digest",
        "has.dot",
        "0abc",
        "lll",
        too_long.as_str(),
    ] {
        get_artifact(
            &server,
            DEFAULT_TENANT,
            DEFAULT_API_KEY,
            DEFAULT_ORG,
            DEFAULT_WORKSPACE,
            DEFAULT_ENV,
            digest,
        )
        .await
        .assert_status_bad_request();

        put_artifact(
            &server,
            DEFAULT_TENANT,
            DEFAULT_API_KEY,
            DEFAULT_ORG,
            DEFAULT_WORKSPACE,
            DEFAULT_ENV,
            digest,
            body.clone(),
        )
        .await
        .assert_status_bad_request();

        head_artifact(
            &server,
            DEFAULT_TENANT,
            DEFAULT_API_KEY,
            DEFAULT_ORG,
            DEFAULT_WORKSPACE,
            DEFAULT_ENV,
            digest,
        )
        .await
        .assert_status_bad_request();

        delete_artifact(
            &server,
            DEFAULT_TENANT,
            DEFAULT_API_KEY,
            DEFAULT_ORG,
            DEFAULT_WORKSPACE,
            DEFAULT_ENV,
            digest,
        )
        .await
        .assert_status_bad_request();
    }

    get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await
    .assert_status_not_found();
}

#[tokio::test]
async fn test_get_artifact_without_read_artifacts_scope() {
    let mut cfg = default_config();

    cfg.security
        .api_keys
        .get_mut(DEFAULT_API_KEY)
        .unwrap()
        .scopes = AllOrSpecificConfiguration::Specific(unordered_set!());

    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_forbidden();
}

#[tokio::test]
async fn test_get_artifacts_without_list_artifacts_scope() {
    let cfg = default_config_with_scopes(unordered_set!());

    let server = create_server(&cfg).await;

    let get_resp = get_artifacts(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
    )
    .await;

    get_resp.assert_status_forbidden();
}

#[tokio::test]
async fn test_put_artifact_without_write_artifacts_scope() {
    let cfg = default_config_with_scopes(unordered_set!());

    let server = create_server(&cfg).await;
    let body = default_body();

    let put_resp = put_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
        body.clone(),
    )
    .await;

    put_resp.assert_status_forbidden();
}

#[tokio::test]
async fn test_delete_artifact_without_delete_artifacts_scope() {
    let cfg = default_config_with_scopes(unordered_set!());

    let server = create_server(&cfg).await;

    let delete_resp = delete_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    delete_resp.assert_status_forbidden();
}

#[tokio::test]
async fn test_request_with_api_key_no_access_to_tenant() {
    let mut cfg = default_config();

    cfg.security
        .api_keys
        .get_mut(DEFAULT_API_KEY)
        .unwrap()
        .tenants = AllOrSpecificConfiguration::Specific(unordered_set!());

    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        "invalid-tenant",
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_forbidden();
}

#[tokio::test]
async fn test_request_with_api_key_no_access_to_org() {
    let mut cfg = default_config();

    cfg.security
        .api_keys
        .get_mut(DEFAULT_API_KEY)
        .unwrap()
        .organizations = AllOrSpecificConfiguration::Specific(unordered_set!());

    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_forbidden();
}

#[tokio::test]
async fn test_request_with_api_key_no_access_to_workspace() {
    let mut cfg = default_config();

    cfg.security
        .api_keys
        .get_mut(DEFAULT_API_KEY)
        .unwrap()
        .workspaces = AllOrSpecificConfiguration::Specific(unordered_set!());

    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_forbidden();
}

#[tokio::test]
async fn test_request_with_api_key_no_access_to_env() {
    let mut cfg = default_config();

    cfg.security
        .api_keys
        .get_mut(DEFAULT_API_KEY)
        .unwrap()
        .environments = AllOrSpecificConfiguration::Specific(unordered_set!());

    let server = create_server(&cfg).await;

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        "invalid-env",
        DEFAULT_DIGEST,
    )
    .await;

    get_resp.assert_status_forbidden();
}
