use std::time::Duration;

use bytesize::ByteSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheStatsParams {
    #[serde(default)]
    pub project: Vec<String>,
    /// Exact task names; glob characters are literal.
    #[serde(default)]
    pub task: Vec<String>,
    /// Task name globs, in addition to `task`.
    #[serde(default)]
    pub task_pattern: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheStatsResult {
    pub projects: Vec<ProjectCacheStatsSummary>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectCacheStatsSummary {
    pub project_name: String,
    pub tasks: Vec<TaskCacheStatsSummary>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskCacheStatsSummary {
    pub task_name: String,
    pub total_size_bytes: u64,
    pub cached_files_count: usize,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CachePruneParams {
    #[serde(default = "default_true")]
    pub dry_run: bool,
    #[serde(default)]
    pub stale_only: bool,
    #[serde(default)]
    pub project: Vec<String>,
    /// Exact task names; glob characters are literal.
    #[serde(default)]
    pub task: Vec<String>,
    /// Task name globs, in addition to `task`.
    #[serde(default)]
    pub task_pattern: Vec<String>,
    /// Only prune entries older than this duration.
    #[serde(default, with = "humantime_serde")]
    #[schemars(with = "String")]
    pub older_than: Option<Duration>,
    #[serde(default)]
    #[schemars(with = "String")]
    pub larger_than: Option<ByteSize>,
    #[serde(default)]
    pub meta: Option<String>,
    #[serde(default)]
    pub dir: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CachePruneResult {
    pub dry_run: bool,
    pub entries_pruned: usize,
    pub bytes_freed: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn stats_task_is_exact_and_task_pattern_is_separate() {
        let params: CacheStatsParams = serde_json::from_value(
            json!({ "task": ["build*"], "task_pattern": ["t*"] }),
        )
        .unwrap();
        assert_eq!(params.task, ["build*"]);
        assert_eq!(params.task_pattern, ["t*"]);
    }

    #[test]
    fn prune_task_is_exact_and_task_pattern_is_separate() {
        let params: CachePruneParams = serde_json::from_value(
            json!({ "task": ["build*"], "task_pattern": ["t*"] }),
        )
        .unwrap();
        assert_eq!(params.task, ["build*"]);
        assert_eq!(params.task_pattern, ["t*"]);
        assert!(params.dry_run, "dry_run still defaults to true");
    }
}
