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
use sha2::{Digest as _, Sha256};

const KEY: &str = "good-key";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

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
        "CREATE TABLE usage(op TEXT, tenant TEXT, digest TEXT, outcome TEXT, api_key_id TEXT, credential_kind TEXT, principal TEXT, namespace TEXT, sha256 TEXT)",
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

fn repository_config(url: &str, after: &str) -> serde_json::Value {
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
            "after": after
        }}
    })
}

/// Records every new fact: `after` binds 13 parameters (op, tenant, org, ws,
/// env, digest, outcome, caller_id, credential_kind, principal, namespace,
/// scope, sha256).
const AFTER: &str =
    "INSERT INTO usage VALUES (?1, ?2, ?6, ?7, ?8, ?9, ?10, ?11, ?13)";

async fn server(url: &str) -> TestServer {
    server_with(url, AFTER).await
}

async fn server_with(url: &str, after: &str) -> TestServer {
    static NEXT: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "orcs-repo-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::write(&path, repository_config(url, after).to_string()).unwrap();

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

const ONE: &str =
    "/api/v1/artifacts/abc?org=o&ws=w&env=e&namespace=isolated&target=isolated";

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
    // `sqlite:` plus a forward-slash path also works with Windows drive paths.
    let url = format!(
        "sqlite:{}?mode=rwc",
        file.to_string_lossy().replace('\\', "/")
    );
    let db = seed(&url).await;
    let s = server(&url).await;

    let call = |r: axum_test::TestRequest, key: &str, tenant: &str| {
        r.add_header("Authorization", format!("Bearer {key}"))
            .add_header("X-OMNI-TENANT", tenant)
    };

    // unknown key is unauthorized; a known key with the wrong tenant is forbidden
    call(s.get(ONE), "bad-key", "t")
        .await
        .assert_status(http::StatusCode::UNAUTHORIZED);
    // Check authorization against an existing resource, not ownership failure.
    db.execute("UPDATE api_keys SET tenant = 'other'", &[])
        .await
        .unwrap();
    call(s.get(ONE), KEY, "t")
        .await
        .assert_status(http::StatusCode::FORBIDDEN);
    db.execute("UPDATE api_keys SET tenant = 't'", &[])
        .await
        .unwrap();

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
        .add_header("X-Omni-Content-Sha256", sha256_hex(b"x"))
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
    // The same facts the stored entry records.
    assert!(
        db.exists(
            "SELECT 1 AS found FROM usage WHERE outcome = 'stored' AND credential_kind = 'api_key' AND principal = ?1 AND namespace = 'isolated' AND sha256 = ?2",
            &[&key_id, &sha256_hex(b"x")],
        )
        .await
        .unwrap(),
        "usage records the credential kind, principal, namespace and hash"
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

/// A hook query written before RFC 0030 binds the first eight parameters. On
/// SQLite it is still accepted, and because those positions keep their meaning
/// it records exactly what it always did.
#[tokio::test]
async fn a_hook_query_written_for_the_older_parameter_list_records_what_it_did()
{
    let file = std::env::temp_dir()
        .join(format!("orcs-older-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let url = format!(
        "sqlite:{}?mode=rwc",
        file.to_string_lossy().replace('\\', "/")
    );
    let db = seed(&url).await;
    db.execute("DROP TABLE usage", &[]).await.unwrap();
    db.execute(
        "CREATE TABLE usage(op TEXT, tenant TEXT, digest TEXT, outcome TEXT, api_key_id TEXT)",
        &[],
    )
    .await
    .unwrap();
    // op, tenant, digest, outcome, api_key_id: the five-column insert of the
    // eight-parameter `after` list.
    let s = server_with(&url, "INSERT INTO usage VALUES (?1, ?2, ?6, ?7, ?8)")
        .await;

    s.put(ONE)
        .add_header("Authorization", format!("Bearer {KEY}"))
        .add_header("X-OMNI-TENANT", "t")
        .add_header("X-Omni-Content-Sha256", sha256_hex(b"x"))
        .bytes("x".into())
        .await
        .assert_status(http::StatusCode::NO_CONTENT);
    let key_id =
        omni_remote_cache_service::hooks::api_key_id_keyed("pepper", KEY);
    let mut recorded = false;
    for _ in 0..50 {
        recorded = db
            .exists(
                "SELECT 1 AS found FROM usage WHERE op = 'put' AND tenant = 't' AND outcome = 'stored' AND api_key_id = ?1",
                &[&key_id],
            )
            .await
            .unwrap();
        if recorded {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        recorded,
        "an older query keeps recording outcome and api_key_id"
    );
}
