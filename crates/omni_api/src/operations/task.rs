use maps::UnorderedMap;
use omni_context::LoadedContext;
use omni_execution_plan::{
    Call, ProjectSelector, ScmAffectedFilter, TaskSelector,
};
use omni_messages::ExecutionEventSubscriber;
use omni_retry_config::RetryConfiguration;
use omni_scm::SelectScm;
use omni_task_executor::{
    ExecutionConfigBuilder, Force, OnFailure, TaskExecutionResult,
    TaskExecutor, TaskExecutorSys,
};
use omni_task_output_logs::LogsDisplay;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Request ─────────────────────────────────────────────────────────────────

/// Filters common to `run` and `exec` operations.
#[derive(Debug, Clone, JsonSchema)]
pub struct TaskRunFilters {
    /// CEL expression to match against task meta configuration.
    pub meta: Option<String>,
    /// What to do when the meta filter fails to evaluate for a task.
    pub meta_on_error: omni_task_executor::MetaOnError,
    /// Failing tasks to list per distinct meta error cause.
    pub meta_error_samples: usize,
    /// Exact project names; glob characters are literal.
    pub project: Vec<String>,
    /// Glob patterns to match project names, in addition to `project`.
    pub project_pattern: Vec<String>,
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
    /// Override the retry settings of all tasks, merged per field over each
    /// task's own configuration.
    pub retry: Option<RetryConfiguration>,
    /// Extra key=value arguments forwarded to tasks.
    pub args: Vec<(String, String)>,
}

impl Default for TaskRunFilters {
    fn default() -> Self {
        Self {
            meta: None,
            meta_on_error: Default::default(),
            meta_error_samples: 3,
            project: vec![],
            project_pattern: vec![],
            dir: vec![],
            max_concurrency: None,
            dry_run: false,
            scm_base: None,
            scm_target: None,
            scm_affected: SelectScm::None,
            retry: None,
            args: vec![],
        }
    }
}

/// Request to run one or more named tasks.
#[derive(Debug, Clone, JsonSchema)]
pub struct TaskRunRequest {
    /// Task name globs to execute.
    pub tasks: Vec<String>,
    /// Task names to execute exactly; glob characters are literal.
    pub exact_tasks: Vec<String>,
    /// Skip dependency resolution and run only the requested tasks.
    pub ignore_dependencies: bool,
    /// Also run the dependents of the matching tasks that have the same task
    /// name, directly or through other tasks of that name. Filters choose the
    /// starting tasks only.
    pub with_dependents: bool,
    /// How to handle task failures.
    #[schemars(with = "String")]
    pub on_failure: OnFailure,
    /// Do not persist results to the local cache.
    pub no_cache: bool,
    /// Do not read from or write to the remote cache for any task.
    #[serde(default)]
    pub no_remote_cache: bool,
    /// Output-logs display policy for fresh (cache-miss) output; `None` inherits
    /// the resolved config (default `failed`).
    pub output_logs: Option<LogsDisplay>,
    /// Output-logs display policy for replayed cached (cache-hit) output;
    /// `None` falls back to `output_logs`, then the resolved config.
    pub output_cached_logs: Option<LogsDisplay>,
    /// Force re-execution even for cached tasks.
    #[schemars(with = "String")]
    pub force: Force,
    /// Filters that narrow down which projects/tasks are in scope.
    pub filters: TaskRunFilters,
}

impl Default for TaskRunRequest {
    fn default() -> Self {
        Self {
            tasks: vec![],
            exact_tasks: vec![],
            ignore_dependencies: false,
            with_dependents: false,
            on_failure: OnFailure::SkipDependents,
            no_cache: false,
            no_remote_cache: false,
            output_logs: None,
            output_cached_logs: None,
            force: Force::None,
            filters: TaskRunFilters::default(),
        }
    }
}

// ── Response ─────────────────────────────────────────────────────────────────

/// Results of a `run` operation.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TaskRunResponse {
    // Results carry executable nodes, so this response is serialize-only;
    // the JSON shape is `{"results": [...]}`, so decode the `results` field as
    // `Vec<TaskExecutionResultReport>` (or a wrapper object with the same shape).
    #[schemars(with = "Vec<serde_json::Value>")]
    pub results: Vec<TaskExecutionResult>,
}

impl TaskRunResponse {
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
pub async fn handle_task_run<TSys, S>(
    ctx: &LoadedContext<TSys>,
    subscriber: &S,
    req: TaskRunRequest,
) -> eyre::Result<TaskRunResponse>
where
    TSys: TaskExecutorSys + Clone,
    S: ExecutionEventSubscriber,
{
    let config = build_run_config(&req)?;
    let executor = TaskExecutor::new(config, ctx, subscriber);
    let results = executor.run().await?;

    Ok(TaskRunResponse { results })
}

/// Cache eligibility of the selected tasks, derived from configuration alone.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskCacheInspectResponse {
    #[schemars(with = "serde_json::Value")]
    pub inspection: omni_task_executor::CacheInspection,
}

/// Inspect the cache status of the tasks `req` selects without running them.
/// Only the selection fields of the request apply.
///
/// Loads a fresh definitions-only view, so no environment value is evaluated
/// and no command substitution can run, even if the API already holds a
/// fully loaded context.
pub async fn handle_task_cache_inspect<TSys>(
    ctx: &omni_context::Context<TSys>,
    req: TaskRunRequest,
) -> eyre::Result<TaskCacheInspectResponse>
where
    TSys: TaskExecutorSys + Clone,
{
    let config = build_run_config(&req)?;
    let loaded = ctx.reload()?.into_loaded_definitions_only().await?;
    let inspection =
        omni_task_executor::inspect_cache_eligibility(&loaded, &config)?;
    Ok(TaskCacheInspectResponse { inspection })
}

fn build_run_config(
    req: &TaskRunRequest,
) -> eyre::Result<omni_task_executor::ExecutionConfig> {
    let mut builder = ExecutionConfigBuilder::default();

    builder
        .ignore_dependencies(req.ignore_dependencies)
        .with_dependents(req.with_dependents)
        .on_failure(req.on_failure)
        .no_cache(req.no_cache)
        .no_remote_cache(req.no_remote_cache)
        .force(req.force)
        .call(Call::Tasks(
            req.tasks
                .iter()
                .map(|pattern| Ok(TaskSelector::pattern(pattern)))
                .chain(
                    req.exact_tasks
                        .iter()
                        .map(|name| TaskSelector::parse_exact(name)),
                )
                .collect::<Result<Vec<_>, _>>()?,
        ));

    if let Some(output_logs) = req.output_logs {
        builder.output_logs(output_logs);
    }
    if let Some(output_cached_logs) = req.output_cached_logs {
        builder.output_cached_logs(output_cached_logs);
    }

    apply_filters(&mut builder, &req.filters);

    Ok(builder.build()?)
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Apply [`RunFilters`] onto an [`ExecutionConfigBuilder`].
pub(crate) fn apply_filters(
    builder: &mut ExecutionConfigBuilder,
    filters: &TaskRunFilters,
) {
    if let Some(meta) = &filters.meta {
        builder.meta_filter(meta);
    }

    builder.meta_on_error(filters.meta_on_error);
    builder.meta_error_samples(filters.meta_error_samples);

    builder.project_filters(
        filters
            .project
            .iter()
            .map(ProjectSelector::exact)
            .chain(filters.project_pattern.iter().map(ProjectSelector::pattern))
            .collect::<Vec<_>>(),
    );
    builder.dir_filters(filters.dir.clone());

    if let Some(max_conc) = filters.max_concurrency {
        builder.max_concurrency(max_conc);
    }

    builder.dry_run(filters.dry_run);

    if let Some(retry) = &filters.retry {
        builder.retry(retry.clone());
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
