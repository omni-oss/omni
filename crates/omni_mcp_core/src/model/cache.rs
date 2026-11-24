use std::time::Duration;

use bytesize::ByteSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheStatsParams {
    /// Exact project names; glob characters are literal.
    #[serde(default)]
    pub project: Vec<String>,
    /// Project name globs, in addition to `project`.
    #[serde(default)]
    pub project_pattern: Vec<String>,
    /// Exact task names; glob characters are literal.
    #[serde(default)]
    pub task: Vec<String>,
    /// Task name globs, in addition to `task`.
    #[serde(default)]
    pub task_pattern: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheEligibilityParams {
    /// Exact names of tasks to inspect: a task or an alias, optionally
    /// `project#task`. Glob characters are literal; use
    /// `task_patterns` to select by glob. At least one of `tasks` and
    /// `task_patterns` is required.
    #[serde(default)]
    pub tasks: Vec<String>,
    /// Glob patterns selecting tasks by name. May be combined with `tasks`.
    #[serde(default)]
    pub task_patterns: Vec<String>,
    /// Exact project names; glob characters are literal.
    #[serde(default)]
    pub project: Vec<String>,
    /// Project name globs, in addition to `project`.
    #[serde(default)]
    pub project_patterns: Vec<String>,
    /// Directory filter.
    #[serde(default)]
    pub dir: Vec<String>,
    /// CEL expression selecting tasks by their `meta` configuration.
    #[serde(default)]
    pub meta: Option<String>,
    /// Inspect only the selected tasks, not the tasks they depend on.
    #[serde(default)]
    pub ignore_dependencies: bool,
}

/// Per-task cache status and explicit `cache.enabled` contradictions.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheEligibilityResult {
    #[schemars(with = "serde_json::Value")]
    pub tasks: Vec<omni_task_executor::InspectedTask>,
    #[schemars(with = "serde_json::Value")]
    pub contradictions: Vec<omni_task_executor::InspectedContradiction>,
    /// Tasks whose `enabled` condition was not evaluated.
    pub unevaluated_conditions: Vec<String>,
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
    #[serde(default, alias = "stale_only")]
    pub stale: bool,
    /// Entries for tasks absent from all cache-sharing worktrees. Conflicts
    /// with stale, dir and meta.
    #[serde(default)]
    pub orphan: bool,
    /// Exact project names; glob characters are literal.
    #[serde(default)]
    pub project: Vec<String>,
    /// Project name globs, in addition to `project`.
    #[serde(default)]
    pub project_pattern: Vec<String>,
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

    #[test]
    fn cache_project_is_exact_and_project_pattern_is_separate() {
        let stats: CacheStatsParams = serde_json::from_value(
            json!({ "project": ["app*"], "project_pattern": ["lib-*"] }),
        )
        .unwrap();
        assert_eq!(stats.project, ["app*"]);
        assert_eq!(stats.project_pattern, ["lib-*"]);
        let prune: CachePruneParams = serde_json::from_value(
            json!({ "project": ["app*"], "project_pattern": ["lib-*"] }),
        )
        .unwrap();
        assert_eq!(prune.project, ["app*"]);
        assert_eq!(prune.project_pattern, ["lib-*"]);
    }
}

#[cfg(test)]
mod eligibility_tests {
    use super::*;

    #[test]
    fn params_default_everything_but_the_selection() {
        let p: CacheEligibilityParams =
            serde_json::from_value(serde_json::json!({"tasks": ["build"]}))
                .unwrap();
        assert_eq!(p.tasks, vec!["build"]);
        assert!(p.task_patterns.is_empty() && !p.ignore_dependencies);
    }

    #[test]
    fn result_keeps_the_inspection_field_names() {
        let r = CacheEligibilityResult {
            tasks: vec![],
            contradictions: vec![],
            unevaluated_conditions: vec!["a#b".into()],
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({
                "tasks": [],
                "contradictions": [],
                "unevaluated_conditions": ["a#b"]
            })
        );
    }
}
