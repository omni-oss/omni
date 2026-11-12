use std::{sync::Arc, time::Duration};

use omni_remote_cache_core::{
    hooks::{BoxTask, HostRuntime},
    providers::DependencyProvider,
};
use omni_remote_cache_repository_query::{
    QueryExecutor, RepositoryConfiguration,
};
use omni_remote_cache_repository_sql::SqlxExecutor;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfiguration {
    /// A `sqlite:`, `postgres:` or `mysql:` url. The driver must be enabled
    /// by a cargo feature of this service (all are by default).
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_acquire_timeout_ms")]
    pub acquire_timeout_ms: u64,
    /// Size of the separate pool used by hooks, so slow hook queries cannot
    /// take connections from authorization lookups. Defaults to 2. Ignored
    /// for in-memory SQLite, which has to share one connection.
    #[serde(default = "default_hooks_max_connections")]
    pub hooks_max_connections: u32,
    /// Remote Postgres and MySQL databases must verify the server certificate
    /// (`sslmode=verify-full` or `verify-ca`; `ssl-mode=VERIFY_IDENTITY` or
    /// `VERIFY_CA`). Set this to opt out, for example on a trusted private
    /// network. Loopback and unix-socket databases are always allowed.
    #[serde(default)]
    pub allow_insecure_tls: bool,
    /// Upper bound for one statement. Defaults to 10000.
    #[serde(default = "default_query_timeout_ms")]
    pub query_timeout_ms: u64,
}

fn default_query_timeout_ms() -> u64 {
    10_000
}

fn default_max_connections() -> u32 {
    5
}

fn default_hooks_max_connections() -> u32 {
    2
}

fn default_acquire_timeout_ms() -> u64 {
    5_000
}

#[derive(Debug, Clone, Deserialize)]
pub struct SqlRepositoryFile {
    pub database: DatabaseConfiguration,
    #[serde(flatten)]
    pub repository: RepositoryConfiguration,
}

struct TokioRuntime;

impl HostRuntime for TokioRuntime {
    fn spawn_detached(&self, task: BoxTask) {
        tokio::spawn(task);
    }

    fn sleep(&self, duration: Duration) -> BoxTask {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// Rejects a remote Postgres or MySQL url that does not verify the server
/// certificate, because sqlx falls back to plaintext by default.
pub fn check_tls(url: &str) -> eyre::Result<()> {
    let parsed = url::Url::parse(url)?;
    let (key, verified): (&str, &[&str]) = match parsed.scheme() {
        "postgres" | "postgresql" => ("sslmode", &["verify-full", "verify-ca"]),
        "mysql" | "mariadb" => ("ssl-mode", &["verify_identity", "verify_ca"]),
        _ => return Ok(()),
    };
    // Non-special schemes such as `postgres:` keep hosts opaque, so IPs
    // arrive as text and have to be parsed here.
    let local = parsed.host_str().is_none_or(|h| {
        let h = h.trim_start_matches('[').trim_end_matches(']');
        h.eq_ignore_ascii_case("localhost")
            || h.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if local {
        return Ok(());
    }
    let mode = parsed
        .query_pairs()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.to_ascii_lowercase());
    if mode.as_deref().is_some_and(|m| verified.contains(&m)) {
        return Ok(());
    }
    eyre::bail!(
        "the database url must set `{key}` to one of {verified:?} for a remote host (or set `allow_insecure_tls`)"
    )
}

pub async fn build_sql_provider(
    path: &str,
) -> eyre::Result<Arc<dyn DependencyProvider>> {
    let mut file: SqlRepositoryFile =
        serde_json::from_str(&std::fs::read_to_string(path)?)?;
    if !file.database.allow_insecure_tls {
        check_tls(&file.database.url)?;
    }
    let query_timeout = Duration::from_millis(file.database.query_timeout_ms);
    let connect = |max| {
        SqlxExecutor::connect(
            &file.database.url,
            max,
            Duration::from_millis(file.database.acquire_timeout_ms),
        )
    };
    let executor = connect(file.database.max_connections)
        .await?
        .with_query_timeout(query_timeout);

    let hooks_executor: Option<Arc<dyn QueryExecutor>> =
        if file.database.url.contains(":memory:")
            || file.database.hooks_max_connections == 0
        {
            None
        } else {
            let hooks = connect(file.database.hooks_max_connections)
                .await?
                .with_query_timeout(query_timeout);
            Some(Arc::new(hooks))
        };
    file.repository
        .validate_queries(&executor, hooks_executor.as_deref())
        .await?;
    // The hooks pool bounds `after` work, so default the in-flight cap to it.
    file.repository
        .hooks
        .after_max_in_flight
        .get_or_insert(file.database.hooks_max_connections.max(1) as usize);

    file.repository.build_provider(
        Arc::new(executor),
        hooks_executor,
        Some(Arc::new(TokioRuntime)),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::check_tls;

    #[test]
    fn remote_databases_must_verify_the_certificate() {
        for ok in [
            "postgres://u:p@db.example.com/d?sslmode=verify-full",
            "postgres://u:p@db.example.com/d?sslmode=verify-ca",
            "mysql://u:p@db.example.com/d?ssl-mode=VERIFY_IDENTITY",
            "mysql://u:p@db.example.com/d?ssl-mode=verify_ca",
            "postgres://u:p@localhost/d",
            "postgres://u:p@127.0.0.1/d",
            "postgres://u:p@[::1]/d",
            "sqlite://file.db",
            "sqlite::memory:",
        ] {
            assert!(check_tls(ok).is_ok(), "{ok}");
        }
        for bad in [
            "postgres://u:p@db.example.com/d",
            "postgres://u:p@db.example.com/d?sslmode=require",
            "postgres://u:p@db.example.com/d?sslmode=prefer",
            "mysql://u:p@db.example.com/d",
            "mysql://u:p@db.example.com/d?ssl-mode=REQUIRED",
        ] {
            assert!(check_tls(bad).is_err(), "{bad}");
        }
    }
}
