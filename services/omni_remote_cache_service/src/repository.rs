use std::{sync::Arc, time::Duration};

use omni_remote_cache_core::{
    hooks::{BoxTask, HostRuntime},
    providers::DependencyProvider,
};
use omni_remote_cache_repository_query::RepositoryConfiguration;
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

pub async fn build_sql_provider(
    path: &str,
) -> eyre::Result<Arc<dyn DependencyProvider>> {
    let mut file: SqlRepositoryFile =
        serde_json::from_str(&std::fs::read_to_string(path)?)?;
    // Hooks share the pool with authorization lookups, so unless set, keep
    // one connection free of `after` work.
    let reserve = file.database.max_connections.saturating_sub(1).max(1);
    file.repository
        .hooks
        .after_max_in_flight
        .get_or_insert(reserve as usize);
    let executor = SqlxExecutor::connect(
        &file.database.url,
        file.database.max_connections,
        Duration::from_millis(file.database.acquire_timeout_ms),
    )
    .await?
    .with_query_timeout(Duration::from_millis(file.database.query_timeout_ms));
    file.repository.build_provider(
        Arc::new(executor),
        Some(Arc::new(TokioRuntime)),
        None,
    )
}
