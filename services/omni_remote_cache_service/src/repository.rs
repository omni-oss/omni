use std::{sync::Arc, time::Duration};

use omni_remote_cache_core::{
    hooks::{Background, BoxTask},
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

struct TokioBackground;

impl Background for TokioBackground {
    fn spawn(&self, task: BoxTask) {
        tokio::spawn(task);
    }

    fn sleep(&self, duration: Duration) -> BoxTask {
        Box::pin(tokio::time::sleep(duration))
    }
}

pub async fn build_sql_provider(
    path: &str,
) -> eyre::Result<Arc<dyn DependencyProvider>> {
    let file: SqlRepositoryFile =
        serde_json::from_str(&std::fs::read_to_string(path)?)?;
    let executor = SqlxExecutor::connect(
        &file.database.url,
        file.database.max_connections,
        Duration::from_millis(file.database.acquire_timeout_ms),
    )
    .await?
    .with_query_timeout(Duration::from_millis(file.database.query_timeout_ms));
    file.repository.build_provider(
        Arc::new(executor),
        Some(Arc::new(TokioBackground)),
        None,
    )
}
