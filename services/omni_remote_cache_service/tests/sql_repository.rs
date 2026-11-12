use std::time::Duration;

use axum_test::TestServer;
use omni_remote_cache_repository_query::{
    ApiKeyHashConfiguration, QueryExecutor,
};
use omni_remote_cache_repository_sql::SqlxExecutor;
use omni_remote_cache_service::{
    args::{BackendType, ConfigType, RouterArgs, ServeArgs},
    routes::root::RouterConfig,
    state::build_state,
};
use serde_json::json;

const KEY: &str = "good-key";

async fn seed(url: &str) -> SqlxExecutor {
    let db = SqlxExecutor::connect(url, 1, Duration::from_secs(5))
        .await
        .unwrap();
    for ddl in [
        "CREATE TABLE tenants(code TEXT)",
        "CREATE TABLE orgs(tenant TEXT, code TEXT)",
        "CREATE TABLE workspaces(org TEXT, code TEXT)",
        "CREATE TABLE envs(ws TEXT, code TEXT)",
        "CREATE TABLE api_keys(key_hash TEXT, tenant TEXT)",
        "CREATE TABLE key_scopes(key_hash TEXT, scope TEXT)",
        "CREATE TABLE vetoes(tenant TEXT, op TEXT, reason TEXT)",
        "CREATE TABLE usage(op TEXT, tenant TEXT, digest TEXT, outcome TEXT, api_key_id TEXT)",
        "INSERT INTO tenants VALUES ('t')",
        "INSERT INTO orgs VALUES ('t','o')",
        "INSERT INTO workspaces VALUES ('o','w')",
        "INSERT INTO envs VALUES ('w','e')",
    ] {
        db.execute(ddl, &[]).await.unwrap();
    }
    let hash = ApiKeyHashConfiguration::default().apply(KEY);
    db.execute("INSERT INTO api_keys VALUES (?1, 't')", &[&hash])
        .await
        .unwrap();
    for scope in ["read:artifacts", "write:artifacts"] {
        db.execute("INSERT INTO key_scopes VALUES (?1, ?2)", &[&hash, scope])
            .await
            .unwrap();
    }
    db
}

fn repository_config(url: &str) -> serde_json::Value {
    json!({
        "database": { "url": url, "max_connections": 1 },
        "queries": {
            "tenant_exists": "SELECT 1 AS found FROM tenants WHERE code = ?1",
            "organization_exists": "SELECT 1 AS found FROM orgs WHERE code = ?1",
            "organization_belongs_to_tenant": "SELECT 1 AS found FROM orgs WHERE tenant = ?1 AND code = ?2",
            "workspace_exists": "SELECT 1 AS found FROM workspaces WHERE code = ?1",
            "workspace_belongs_to_organization": "SELECT 1 AS found FROM workspaces WHERE org = ?1 AND code = ?2",
            "environment_exists": "SELECT 1 AS found FROM envs WHERE code = ?1",
            "environment_belongs_to_workspace": "SELECT 1 AS found FROM envs WHERE ws = ?1 AND code = ?2",
            "api_key_valid": "SELECT 1 AS found FROM api_keys WHERE key_hash = ?1 LIMIT 1",
            "api_key_tenant": "SELECT 1 AS found FROM api_keys WHERE key_hash = ?1 AND tenant = ?2",
            "api_key_organization": "SELECT (COUNT(*) > 0) AS found FROM api_keys WHERE key_hash = ?1",
            "api_key_workspace": "SELECT (COUNT(*) > 0) AS found FROM api_keys WHERE key_hash = ?1",
            "api_key_environment": "SELECT (COUNT(*) > 0) AS found FROM api_keys WHERE key_hash = ?1",
            "api_key_scope": "SELECT 1 AS found FROM key_scopes WHERE key_hash = ?1 AND scope = ?2"
        },
        "hooks": { "api_key_id_secret": "pepper", "queries": {
            "before": "SELECT reason AS veto_reason FROM vetoes WHERE tenant = ?2 AND (op = ?1 OR op = '*') LIMIT 1",
            "after": "INSERT INTO usage VALUES (?1, ?2, ?6, ?7, ?8)"
        }}
    })
}

async fn server(url: &str) -> TestServer {
    let path = std::env::temp_dir().join(format!(
        "orcs-repo-{}.json",
        url.rsplit('/').next().unwrap()
    ));
    std::fs::write(&path, repository_config(url).to_string()).unwrap();

    let args = ServeArgs::new(
        "".to_string(),
        None,
        Some(ConfigType::File),
        false,
        None,
        None,
        Some(100),
        BackendType::InMemory,
        Some(RouterArgs::new(Some("/api".to_string()), false)),
        Some(path.to_string_lossy().to_string()),
    );
    let state = build_state(&args).await.unwrap();
    let router = omni_remote_cache_service::routes::root::build_router(
        &RouterConfig::from(args.routes.clone().unwrap()),
    )
    .with_state(state);
    TestServer::new(router)
}

const ONE: &str = "/api/v1/artifacts/abc?org=o&ws=w&env=e";

async fn usage_rows(db: &SqlxExecutor, outcome: &str) -> bool {
    for _ in 0..50 {
        if db
            .exists(
                "SELECT 1 AS found FROM usage WHERE outcome = ?1 LIMIT 1",
                &[outcome],
            )
            .await
            .unwrap()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn sql_backend_authorizes_validates_vetoes_and_records_usage() {
    let file =
        std::env::temp_dir().join(format!("orcs-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let db = seed(&url).await;
    let s = server(&url).await;

    let call = |r: axum_test::TestRequest, key: &str, tenant: &str| {
        r.add_header("X-API-KEY", key)
            .add_header("X-OMNI-TENANT", tenant)
    };

    // unknown key and wrong tenant are forbidden
    call(s.get(ONE), "bad-key", "t")
        .await
        .assert_status(http::StatusCode::FORBIDDEN);
    call(s.get(ONE), KEY, "other")
        .await
        .assert_status(http::StatusCode::FORBIDDEN);

    // a scope the key lacks is forbidden
    call(s.delete(ONE), KEY, "t")
        .await
        .assert_status(http::StatusCode::FORBIDDEN);

    // unknown ownership is a validation error
    call(s.get("/api/v1/artifacts/abc?org=nope&ws=w&env=e"), KEY, "t")
        .await
        .assert_status(http::StatusCode::BAD_REQUEST);

    // put, then the after hook records usage off the request path
    call(s.put(ONE), KEY, "t")
        .bytes("x".into())
        .await
        .assert_status(http::StatusCode::NO_CONTENT);
    assert!(usage_rows(&db, "stored").await);
    let key_id =
        omni_remote_cache_service::hooks::api_key_id_keyed("pepper", KEY);
    assert_ne!(key_id, omni_remote_cache_service::hooks::api_key_id(KEY));
    assert!(
        db.exists(
            "SELECT 1 AS found FROM usage WHERE outcome = 'stored' AND api_key_id = ?1",
            &[&key_id],
        )
        .await
        .unwrap(),
        "usage is attributed to the key's opaque id"
    );

    call(s.get(ONE), KEY, "t")
        .await
        .assert_status(http::StatusCode::OK);
    assert!(usage_rows(&db, "hit").await);

    // a veto row blocks the operation with its reason
    db.execute(
        "INSERT INTO vetoes VALUES ('t', 'get', 'pruning in progress')",
        &[],
    )
    .await
    .unwrap();
    let res = call(s.get(ONE), KEY, "t").await;
    res.assert_status(http::StatusCode::FORBIDDEN);
    assert!(res.text().contains("pruning in progress"));

    let _ = std::fs::remove_file(&file);
}
