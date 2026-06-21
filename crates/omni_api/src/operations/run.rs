use std::time::Duration;

use maps::UnorderedMap;
use omni_context::LoadedContext;
use omni_execution_plan::{Call, ScmAffectedFilter};
use omni_messages::ExecutionEventSubscriber;
use omni_scm::SelectScm;
use omni_task_executor::{
    ExecutionConfigBuilder, Force, OnFailure, TaskExecutionResult,
    TaskExecutor, TaskExecutorSys,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Request ─────────────────────────────────────────────────────────────────

/// Filters common to `run` and `exec` operations.
#[derive(Debug, Clone, JsonSchema)]
pub struct RunFilters {
    /// CEL expression to match against task meta configuration.
    pub meta: Option<String>,
    /// Glob patterns to match project names.
    pub project: Vec<String>,
    /// Glob patterns to match project directories.
    pub dir: Vec<String>,
    /// Maximum number of tasks to run concurrently.
    pub max_concurrency: Option<usize>,
    /// Print commands instead of executing them.
    pub dry_run: bool,
    /// SCM base commit for affected-files filtering.
    pub scm_base: Option<String>,
    /// SCM target commit for affected-files filtering.
    pub scm_target: Option<String>,
    /// SCM strategy for affected-files detection.
    #[schemars(with = "String")]
    pub scm_affected: SelectScm,
    /// Override max retries for all tasks.
    pub retry: Option<u8>,
    /// Override retry interval for all tasks.
    pub retry_interval: Option<Duration>,
    /// Extra key=value arguments forwarded to tasks.
    pub args: Vec<(String, String)>,
}

impl Default for RunFilters {
    fn default() -> Self {
        Self {
            meta: None,
            project: vec![],
            dir: vec![],
            max_concurrency: None,
            dry_run: false,
            scm_base: None,
            scm_target: None,
            scm_affected: SelectScm::None,
            retry: None,
            retry_interval: None,
            args: vec![],
        }
    }
}

/// Request to run one or more named tasks.
#[derive(Debug, Clone, JsonSchema)]
pub struct RunRequest {
    /// Task names to execute.
    pub tasks: Vec<String>,
    /// Skip dependency resolution and run only the requested tasks.
    pub ignore_dependencies: bool,
    /// Also run tasks that depend on the matching tasks.
    pub with_dependents: bool,
    /// How to handle task failures.
    #[schemars(with = "String")]
    pub on_failure: OnFailure,
    /// Do not persist results to the local cache.
    pub no_cache: bool,
    /// Do not replay cached task output logs.
    pub no_replay_logs: bool,
    /// Force re-execution even for cached tasks.
    #[schemars(with = "String")]
    pub force: Force,
    /// Filters that narrow down which projects/tasks are in scope.
    pub filters: RunFilters,
}

impl Default for RunRequest {
    fn default() -> Self {
        Self {
            tasks: vec![],
            ignore_dependencies: false,
            with_dependents: false,
            on_failure: OnFailure::SkipDependents,
            no_cache: false,
            no_replay_logs: false,
            force: Force::None,
            filters: RunFilters::default(),
        }
    }
}

// ── Response ─────────────────────────────────────────────────────────────────

/// Results of a `run` operation.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RunResponse {
    // `TaskExecutionResult` lives in `omni_task_executor` and does not
    // implement `JsonSchema`; represent it opaquely here.
    #[schemars(with = "Vec<serde_json::Value>")]
    pub results: Vec<TaskExecutionResult>,
}

impl RunResponse {
    /// Returns `true` if every task either succeeded or was a clean cache hit.
    pub fn is_success(&self) -> bool {
        !self.results.iter().any(|r| r.is_failure())
    }
}

// ── Handler ──────────────────────────────────────────────────────────────────

/// Execute one or more named tasks.
///
/// `subscriber` is passed by reference; `&S` implements
/// [`ExecutionEventSubscriber`] via the blanket impl in `omni_messages`.
pub async fn handle_run<TSys, S>(
    ctx: &LoadedContext<TSys>,
    subscriber: &S,
    req: RunRequest,
) -> eyre::Result<RunResponse>
where
    TSys: TaskExecutorSys + Clone,
    S: ExecutionEventSubscriber,
{
    let mut builder = ExecutionConfigBuilder::default();

    builder
        .ignore_dependencies(req.ignore_dependencies)
        .with_dependents(req.with_dependents)
        .on_failure(req.on_failure)
        .no_cache(req.no_cache)
        .force(req.force)
        .replay_cached_logs(!req.no_replay_logs)
        .call(Call::new_tasks(&req.tasks[..]));

    apply_filters(&mut builder, &req.filters);

    let config = builder.build()?;
    let executor = TaskExecutor::new(config, ctx, subscriber);
    let results = executor.run().await?;

    Ok(RunResponse { results })
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Apply [`RunFilters`] onto an [`ExecutionConfigBuilder`].
pub(crate) fn apply_filters(
    builder: &mut ExecutionConfigBuilder,
    filters: &RunFilters,
) {
    if let Some(meta) = &filters.meta {
        builder.meta_filter(meta);
    }

    builder.project_filters(filters.project.clone());
    builder.dir_filters(filters.dir.clone());

    if let Some(max_conc) = filters.max_concurrency {
        builder.max_concurrency(max_conc);
    }

    builder.dry_run(filters.dry_run);

    if let Some(retry) = filters.retry {
        builder.max_retries(retry);
    }

    if let Some(retry_interval) = filters.retry_interval {
        builder.retry_interval(retry_interval);
    }

    let mut scm = filters.scm_affected;
    if scm.is_none()
        && (filters.scm_base.is_some() || filters.scm_target.is_some())
    {
        scm = SelectScm::Auto;
    }
    if !scm.is_none() {
        builder.scm_affected_filter(ScmAffectedFilter {
            base: filters.scm_base.clone(),
            scm,
            target: filters.scm_target.clone(),
        });
    }

    if !filters.args.is_empty() {
        let args: UnorderedMap<String, serde_json::Value> = filters
            .args
            .iter()
            .cloned()
            .map(|(k, v)| (k, serde_json::Value::String(v)))
            .collect();
        builder.args(args);
    }
}
