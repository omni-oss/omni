use maps::UnorderedMap;
use omni_context::LoadedContext;
use omni_execution_plan::{Call, ScmAffectedFilter};
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
            ignore_dependencies: false,
            with_dependents: false,
            on_failure: OnFailure::SkipDependents,
            no_cache: false,
            output_logs: None,
            output_cached_logs: None,
            force: Force::None,
            filters: TaskRunFilters::default(),
        }
    }
}

// ── Response ─────────────────────────────────────────────────────────────────

/// Results of a `run` operation.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskRunResponse {
    // `TaskExecutionResult` lives in `omni_task_executor` and does not
    // implement `JsonSchema`; represent it opaquely here.
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
    let mut builder = ExecutionConfigBuilder::default();

    builder
        .ignore_dependencies(req.ignore_dependencies)
        .with_dependents(req.with_dependents)
        .on_failure(req.on_failure)
        .no_cache(req.no_cache)
        .force(req.force)
        .call(Call::new_tasks(&req.tasks[..]));

    if let Some(output_logs) = req.output_logs {
        builder.output_logs(output_logs);
    }
    if let Some(output_cached_logs) = req.output_cached_logs {
        builder.output_cached_logs(output_cached_logs);
    }

    apply_filters(&mut builder, &req.filters);

    let config = builder.build()?;
    let executor = TaskExecutor::new(config, ctx, subscriber);
    let results = executor.run().await?;

    Ok(TaskRunResponse { results })
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

    builder.project_filters(filters.project.clone());
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
