//! The whole service against a real Postgres or MySQL server: identity,
//! scopes, ownership, hooks with their 11 and 13 parameters, and the
//! `principal` column. Skipped unless `TEST_POSTGRES_URL` or `TEST_MYSQL_URL`
//! is set, for example `postgres://postgres:p@127.0.0.1:5432/postgres`.

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

#[derive(Clone, Copy)]
enum Dialect {
    Postgres,
    Mysql,
}

impl Dialect {
    /// The placeholder for parameter `i` (1 based). MySQL's `?` is positional,
    /// so queries must use their parameters in order.
    fn p(self, i: usize) -> String {
        match self {
            Self::Postgres => format!("${i}"),
            Self::Mysql => "?".into(),
        }
    }

    /// A reference that uses parameter `i` without changing a result (every
    /// bound parameter has to appear in the statement).
    fn used(self, i: usize) -> String {
        match self {
            Self::Postgres => format!("${i}::text IS NOT NULL"),
            Self::Mysql => "? IS NOT NULL".into(),
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn seed(
    url: &str,
    d: Dialect,
    suffix: &str,
) -> (SqlxExecutor, serde_json::Value) {
    let t = |name: &str| format!("{name}_{suffix}");
    let db = SqlxExecutor::connect(url, 2, Duration::from_secs(10))
        .await
        .unwrap();
    for ddl in [
        format!("CREATE TABLE {}(code VARCHAR(64))", t("tenants")),
        format!(
            "CREATE TABLE {}(tenant VARCHAR(64), code VARCHAR(64))",
            t("orgs")
        ),
        format!(
            "CREATE TABLE {}(org VARCHAR(64), code VARCHAR(64))",
            t("workspaces")
        ),
        format!(
            "CREATE TABLE {}(ws VARCHAR(64), code VARCHAR(64))",
            t("envs")
        ),
        format!(
            "CREATE TABLE {}(key_hash VARCHAR(128), tenant VARCHAR(64), principal VARCHAR(256))",
            t("api_keys")
        ),
        format!(
            "CREATE TABLE {}(key_hash VARCHAR(128), scope VARCHAR(64))",
            t("key_scopes")
        ),
        format!(
            "CREATE TABLE {}(tenant VARCHAR(64), op VARCHAR(32), reason VARCHAR(256))",
            t("vetoes")
        ),
        format!(
            "CREATE TABLE {}(op VARCHAR(32), tenant VARCHAR(64), digest VARCHAR(128), outcome VARCHAR(32), api_key_id VARCHAR(64), credential_kind VARCHAR(32), principal VARCHAR(256), namespace VARCHAR(16), sha256 VARCHAR(64))",
            t("usage")
        ),
        format!("INSERT INTO {} VALUES ('t')", t("tenants")),
        format!("INSERT INTO {} VALUES ('t','o')", t("orgs")),
        format!("INSERT INTO {} VALUES ('o','w')", t("workspaces")),
        format!("INSERT INTO {} VALUES ('w','e')", t("envs")),
    ] {
        db.execute(&ddl, &[]).await.unwrap();
    }
    let hash = ApiKeyHashConfiguration::default().apply(KEY);
    db.execute(
        &format!(
            "INSERT INTO {} VALUES ({}, 't', 'ci-bot')",
            t("api_keys"),
            if matches!(d, Dialect::Postgres) {
                "$1"
            } else {
                "?"
            }
        ),
        &[&hash],
    )
    .await
    .unwrap();
    for scope in ["read:artifacts", "write:artifacts"] {
        let sql = format!(
            "INSERT INTO {} VALUES ({}, {})",
            t("key_scopes"),
            d.p(1),
            d.p(2)
        );
        db.execute(&sql, &[&hash, scope]).await.unwrap();
    }
    let (p1, p2) = (d.p(1), d.p(2));
    let q = |s: String| s;
    let config = json!({
        "database": { "url": url, "max_connections": 2 },
        "queries": {
            "tenant_exists": q(format!("SELECT 1 AS found FROM {} WHERE code = {p1}", t("tenants"))),
            "organization_exists": q(format!("SELECT 1 AS found FROM {} WHERE code = {p1}", t("orgs"))),
            "organization_belongs_to_tenant": q(format!("SELECT 1 AS found FROM {} WHERE tenant = {p1} AND code = {p2}", t("orgs"))),
            "workspace_exists": q(format!("SELECT 1 AS found FROM {} WHERE code = {p1}", t("workspaces"))),
            "workspace_belongs_to_organization": q(format!("SELECT 1 AS found FROM {} WHERE org = {p1} AND code = {p2}", t("workspaces"))),
            "environment_exists": q(format!("SELECT 1 AS found FROM {} WHERE code = {p1}", t("envs"))),
            "environment_belongs_to_workspace": q(format!("SELECT 1 AS found FROM {} WHERE ws = {p1} AND code = {p2}", t("envs"))),
            // The principal column is read from this query.
            "api_key_valid": q(format!("SELECT 1 AS found, principal FROM {} WHERE key_hash = {p1} LIMIT 1", t("api_keys"))),
            "api_key_tenant": q(format!("SELECT 1 AS found FROM {} WHERE key_hash = {p1} AND tenant = {p2}", t("api_keys"))),
            "api_key_organization": q(format!("SELECT (COUNT(*) > 0) AS found FROM {} WHERE key_hash = {p1} AND {} AND {}", t("api_keys"), d.used(2), d.used(3))),
            "api_key_workspace": q(format!("SELECT (COUNT(*) > 0) AS found FROM {} WHERE key_hash = {p1} AND {} AND {} AND {}", t("api_keys"), d.used(2), d.used(3), d.used(4))),
            "api_key_environment": q(format!("SELECT (COUNT(*) > 0) AS found FROM {} WHERE key_hash = {p1} AND {} AND {} AND {} AND {}", t("api_keys"), d.used(2), d.used(3), d.used(4), d.used(5))),
            "api_key_scope": q(format!("SELECT 1 AS found FROM {} WHERE key_hash = {p1} AND scope = {p2}", t("key_scopes"))),
        },
        "hooks": { "api_key_id_secret": "pepper", "queries": {
            // before binds 11 parameters, after binds 13: every one is used.
            "before": before_query(d, &t("vetoes")),
            "after": after_query(d, &t("usage")),
        }}
    });
    (db, config)
}

fn before_query(d: Dialect, vetoes: &str) -> String {
    match d {
        Dialect::Postgres => {
            let mut q = format!(
                "SELECT reason AS veto_reason FROM {vetoes} WHERE tenant = $2 AND (op = $1 OR op = '*')"
            );
            for i in 3..=11 {
                q.push_str(&format!(" AND ${i}::text IS NOT NULL"));
            }
            q + " LIMIT 1"
        }
        // `?` binds in order: op, tenant, then the nine others.
        Dialect::Mysql => format!(
            "SELECT reason AS veto_reason FROM {vetoes} WHERE (op = ? OR op = '*') AND tenant = ?{} LIMIT 1",
            " AND ? IS NOT NULL".repeat(9)
        ),
    }
}

fn after_query(d: Dialect, usage: &str) -> String {
    // op, tenant, org, ws, env, digest, outcome, caller id, kind, principal,
    // namespace, scope, sha256. Record op, tenant, digest, outcome, caller id,
    // kind, principal, namespace and sha256.
    match d {
        Dialect::Postgres => format!(
            "INSERT INTO {usage} SELECT $1, $2, $6, $7, $8, $9, $10, $11, $13 WHERE $3::text IS NOT NULL AND $4::text IS NOT NULL AND $5::text IS NOT NULL AND $12::text IS NOT NULL"
        ),
        // MySQL's `?` cannot be reused or skipped; one placeholder per
        // parameter, in order, so the skipped ones go in a harmless predicate.
        Dialect::Mysql => format!(
            "INSERT INTO {usage} SELECT a, b, f, g, h, i, j, k, m FROM (SELECT ? AS a, ? AS b, ? AS c, ? AS d, ? AS e, ? AS f, ? AS g, ? AS h, ? AS i, ? AS j, ? AS k, ? AS l, ? AS m) AS params"
        ),
    }
}

async fn server(config: &serde_json::Value, tag: &str) -> TestServer {
    let path = std::env::temp_dir()
        .join(format!("orcs-live-{}-{tag}.json", std::process::id()));
    std::fs::write(&path, config.to_string()).unwrap();
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

async fn run(url: &str, d: Dialect, tag: &str) {
    let suffix = format!("{}_{}", tag, std::process::id());
    let (db, config) = seed(url, d, &suffix).await;
    let s = server(&config, tag).await;
    let call = |r: axum_test::TestRequest, key: &str| {
        r.add_header("Authorization", format!("Bearer {key}"))
            .add_header("X-OMNI-TENANT", "t")
    };

    call(s.get(ONE), "bad-key")
        .await
        .assert_status(http::StatusCode::UNAUTHORIZED);
    call(s.delete(ONE), KEY)
        .await
        .assert_status(http::StatusCode::FORBIDDEN);
    call(s.get("/api/v1/artifacts/abc?org=nope&ws=w&env=e"), KEY)
        .await
        .assert_status(http::StatusCode::BAD_REQUEST);
    call(s.put(ONE), KEY)
        .add_header("X-Omni-Content-Sha256", sha256_hex(b"x"))
        .bytes("x".into())
        .await
        .assert_status(http::StatusCode::NO_CONTENT);
    call(s.get(ONE), KEY)
        .await
        .assert_status(http::StatusCode::OK);

    // The after hook ran with all 13 parameters and recorded the caller's
    // principal from the `principal` column and the namespace.
    let usage = format!("usage_{suffix}");
    let mut ok = false;
    for _ in 0..100 {
        let probe = format!(
            "SELECT 1 AS found FROM {usage} WHERE outcome = 'stored' AND principal = 'ci-bot' AND namespace = 'isolated' AND sha256 = '{}' LIMIT 1",
            sha256_hex(b"x")
        );
        if db.exists(&probe, &[]).await.unwrap() {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        ok,
        "the after hook should record the principal, namespace and hash"
    );
    for t in [
        "tenants",
        "orgs",
        "workspaces",
        "envs",
        "api_keys",
        "key_scopes",
        "vetoes",
        "usage",
    ] {
        let _ = db.execute(&format!("DROP TABLE {t}_{suffix}"), &[]).await;
    }
}

#[tokio::test]
async fn postgres() {
    if let Ok(url) = std::env::var("TEST_POSTGRES_URL") {
        run(&url, Dialect::Postgres, "pg").await;
    }
}

#[tokio::test]
async fn mysql() {
    if let Ok(url) = std::env::var("TEST_MYSQL_URL") {
        run(&url, Dialect::Mysql, "my").await;
    }
}

/// A hook query written for the older, shorter parameter lists is refused at
/// startup on a real server (it binds more values than the query uses).
#[tokio::test]
async fn a_stale_hook_query_stops_the_service_from_starting() {
    for (var, d, tag) in [
        ("TEST_POSTGRES_URL", Dialect::Postgres, "pgs"),
        ("TEST_MYSQL_URL", Dialect::Mysql, "mys"),
    ] {
        let Ok(url) = std::env::var(var) else {
            continue;
        };
        let suffix = format!("{tag}_{}", std::process::id());
        let (_db, mut config) = seed(&url, d, &suffix).await;
        let usage = format!("usage_{suffix}");
        let old = match d {
            Dialect::Postgres => format!(
                "INSERT INTO {usage} SELECT $1, $2, $6, $7, $8, NULL, NULL, NULL, NULL"
            ),
            Dialect::Mysql => format!(
                "INSERT INTO {usage} SELECT ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL"
            ),
        };
        config["hooks"]["queries"]["after"] = serde_json::Value::String(old);
        let path = std::env::temp_dir()
            .join(format!("orcs-stale-{}-{tag}.json", std::process::id()));
        std::fs::write(&path, config.to_string()).unwrap();
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
        assert!(
            build_state(&args).await.is_err(),
            "{var}: a stale query must be rejected"
        );
    }
}
