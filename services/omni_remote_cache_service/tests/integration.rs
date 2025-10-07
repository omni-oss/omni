use axum_test::{TestResponse, TestServer};
use bytes::Bytes;
use http::StatusCode;
use maps::unordered_map;
use omni_remote_cache_service::{
    args::{BackendType, ConfigType, ServeArgs},
    config::{
        All, AllOrSpecificConfiguration, ApiKeyConfiguration, Configuration,
        EnvironmentConfiguration, OrganizationConfiguration,
        ScopesConfiguration, SecurityConfiguration, TenantConfiguration,
        WorkspaceConfiguration,
    },
    response::data::Data,
    routes::{root::RouterConfig, v1::artifacts::CacheItem},
    state::ServiceState,
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
            api_keys: unordered_map!(
                "test-api-key".to_string() => ApiKeyConfiguration {
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

    let route_config = RouterConfig::new(Some("/api".to_string()), true);

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
    );
    let state = ServiceState::from_args(&serve_args)
        .await
        .expect("must be able to construct state");
    let router = omni_remote_cache_service::routes::root::build_router(
        serve_args.routes.as_ref().unwrap(),
    )
    .with_state(state);

    TestServer::new(router).expect("should be able to create test server")
}

const DEFAULT_DIGEST: &str =
    "d8e8fca2dc0f896fd7cb4cb0031ba249274155f46f29cef4e282d744";

const DEFAULT_ORG: &str = "test-org";
const DEFAULT_TENANT: &str = "test-tenant";
const DEFAULT_WORKSPACE: &str = "test-workspace";
const DEFAULT_ENV: &str = "test-env";
const DEFAULT_API_KEY: &str = "test-api-key";

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
        .add_header("X-API-KEY", api_key)
        .add_header("X-OMNI-TENANT", tenant)
        .await
}

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
        .add_header("X-API-KEY", api_key)
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
        .add_header("X-API-KEY", api_key)
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
        .add_header("X-API-KEY", api_key)
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

    get_resp.assert_status_forbidden();
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

    let get_resp = get_artifact(
        &server,
        DEFAULT_TENANT,
        DEFAULT_API_KEY,
        DEFAULT_ORG,
        DEFAULT_WORKSPACE,
        DEFAULT_ENV,
        "invalid-digest",
    )
    .await;

    get_resp.assert_status_not_found();
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
