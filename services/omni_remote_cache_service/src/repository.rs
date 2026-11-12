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
    /// Remote Postgres and MySQL databases must verify the server's identity
    /// (`sslmode=verify-full`, `ssl-mode=VERIFY_IDENTITY`). Set this to opt out, for example on a trusted private
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

/// Rejects a remote Postgres or MySQL url that does not verify the server's
/// identity, because sqlx falls back to plaintext by default.
///
/// This checks what sqlx will apply, not the first thing it sees: the last
/// TLS-mode parameter wins, and Postgres `host` and `hostaddr` parameters
/// replace the url's host. `verify-ca` is not accepted because it does not
/// check the host name.
pub fn check_tls(url: &str) -> eyre::Result<()> {
    check_tls_with(url, |k| std::env::var(k).ok())
}

/// `check_tls` with an explicit environment, for tests.
fn check_tls_with(
    url: &str,
    env: impl Fn(&str) -> Option<String>,
) -> eyre::Result<()> {
    let parsed = url::Url::parse(url)?;
    let verified: &[&str] = match parsed.scheme() {
        "postgres" | "postgresql" => &["verify-full"],
        "mysql" | "mariadb" => &["verify_identity"],
        _ => return Ok(()),
    };

    let is_local = |h: &str| {
        let h = h.trim_start_matches('[').trim_end_matches(']');
        // an empty host or a path is a unix socket
        h.is_empty()
            || h.starts_with('/')
            || h.eq_ignore_ascii_case("localhost")
            || h.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    };
    // Non-special schemes such as `postgres:` keep hosts opaque, so IPs
    // arrive as text and have to be parsed here.
    let mut hosts: Vec<String> =
        parsed.host_str().map(String::from).into_iter().collect();
    let mut mode = None;
    for (k, v) in parsed.query_pairs() {
        // sqlx matches parameter names case-sensitively and ignores others.
        match &*k {
            "host" | "hostaddr" => {
                hosts.extend(v.split(',').map(|h| h.trim().to_string()))
            }
            "sslmode" | "ssl-mode" => mode = Some(v.to_ascii_lowercase()),
            _ => {}
        }
    }

    // With no host at all, sqlx's Postgres driver falls back to PGHOST and
    // PGHOSTADDR before using a local socket.
    if hosts.is_empty() && parsed.scheme().starts_with("postgres") {
        for var in ["PGHOST", "PGHOSTADDR"] {
            if let Some(v) = env(var) {
                hosts.extend(v.split(',').map(|h| h.trim().to_string()));
            }
        }
    }
    if hosts.iter().all(|h| is_local(h)) {
        return Ok(());
    }
    if mode.as_deref().is_some_and(|m| verified.contains(&m)) {
        return Ok(());
    }
    eyre::bail!(
        "a remote database url must set the TLS mode to one of {verified:?} (`sslmode` or `ssl-mode`, last value wins), or set `allow_insecure_tls`"
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
    use super::{check_tls, check_tls_with};

    #[test]
    fn remote_databases_must_verify_the_server_identity() {
        for ok in [
            "postgres://u:p@db.example.com/d?sslmode=verify-full",
            "postgres://u:p@db.example.com/d?sslmode=disable&sslmode=verify-full",
            "mysql://u:p@db.example.com/d?ssl-mode=VERIFY_IDENTITY",
            "postgres://u:p@localhost/d",
            "postgres://u:p@127.0.0.1/d",
            "postgres://u:p@[::1]/d",
            "postgres:///d?host=/var/run/postgresql",
            "sqlite://file.db",
            "sqlite::memory:",
        ] {
            assert!(check_tls(ok).is_ok(), "{ok}");
        }
        for bad in [
            "postgres://u:p@db.example.com/d",
            "postgres://u:p@db.example.com/d?sslmode=require",
            "postgres://u:p@db.example.com/d?sslmode=prefer",
            // verify-ca does not check the host name
            "postgres://u:p@db.example.com/d?sslmode=verify-ca",
            "mysql://u:p@db.example.com/d?ssl-mode=VERIFY_CA",
            "mysql://u:p@db.example.com/d?ssl-mode=REQUIRED",
            // the last mode wins
            "postgres://u:p@db.example.com/d?sslmode=verify-full&sslmode=disable",
            // host and hostaddr replace the url's host
            "postgres://localhost/d?host=db.example.com&sslmode=disable",
            "postgres://localhost/d?hostaddr=10.0.0.5",
            // sqlx ignores a differently cased name, so the mode stays disable
            "postgres://db.example.com/d?sslmode=disable&SSLMODE=verify-full",
            "postgres://db.example.com/d?SSLMODE=verify-full",
            // a differently cased `host` is ignored by sqlx, but `hostaddr`
            // is not
            "postgres://localhost/d?HOSTADDR=10.0.0.5&hostaddr=10.0.0.5",
        ] {
            assert!(check_tls(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_missing_host_follows_the_postgres_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert!(check_tls_with("postgres:///d", env(&[])).is_ok());
        assert!(
            check_tls_with("postgres:///d", env(&[("PGHOST", "localhost")]))
                .is_ok()
        );
        assert!(
            check_tls_with(
                "postgres:///d",
                env(&[("PGHOST", "db.example.com")])
            )
            .is_err()
        );
        assert!(
            check_tls_with("postgres:///d", env(&[("PGHOSTADDR", "10.0.0.5")]))
                .is_err()
        );
        assert!(
            check_tls_with(
                "postgres:///d?sslmode=verify-full",
                env(&[("PGHOST", "db.example.com")])
            )
            .is_ok()
        );
        // an explicit host is not replaced by the environment
        assert!(
            check_tls_with(
                "postgres://localhost/d",
                env(&[("PGHOST", "db.example.com")])
            )
            .is_ok()
        );
    }
}
