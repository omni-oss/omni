use std::time::Duration;

use bytesize::ByteSize;
use derive_new::new;
use maps::UnorderedMap;
use omni_cache::{
    CacheStats, CacheStatsArgs, Context as CacheContext, PruneCacheArgs,
    PrunedCacheEntry, TaskExecutionCacheStore,
};
use omni_configurations::MetaConfiguration;
use omni_context::{
    Context, ContextSys, EnvVarsMap, LoadedContext, LoadedContextError,
};
use omni_core::{Project, ProjectGraph, TaskExecutionNode};
use omni_task_context::CacheInfo;
use omni_task_executor::TaskExecutorSys;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Request types ─────────────────────────────────────────────────────────────

/// Filters for [`handle_cache_stats`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CacheStatsRequest {
    pub project: Vec<String>,
    pub task: Vec<String>,
    pub dir: Vec<String>,
    pub meta: Option<String>,
}

/// Filters for [`handle_cache_prune`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CachePruneRequest {
    /// Only prune cache entries whose digest is stale.
    pub stale_only: bool,
    /// Only prune entries older than this duration.
    pub older_than: Option<Duration>,
    /// Only prune entries larger than this byte size.
    #[schemars(with = "Option<u64>")]
    pub larger_than: Option<ByteSize>,
    pub project: Vec<String>,
    pub task: Vec<String>,
    pub meta: Option<String>,
    pub dir: Vec<String>,
    /// If `true`, only compute and return what *would* be pruned — nothing is deleted.
    pub dry_run: bool,
}

/// Parameters for [`handle_cache_remote_setup`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CacheRemoteSetupRequest {
    pub api_base_url: String,
    pub api_key: String,
    pub tenant: String,
    pub org: String,
    pub ws: String,
    pub env: Option<String>,
    /// Encrypt the remote cache configuration file.
    pub secure: bool,
}

// ── Response types ────────────────────────────────────────────────────────────

/// Result of a [`handle_cache_prune`] call.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CachePruneResponse {
    /// The entries that were (or would be, when `dry_run == true`) pruned.
    // `PrunedCacheEntry` lives in `omni_cache` and does not implement
    // `JsonSchema` (it embeds external types); represent it opaquely here.
    pub entries: Vec<PrunedCacheEntry>,
    /// Mirrors `CachePruneRequest::dry_run`.
    pub dry_run: bool,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// Show per-project cache statistics.
pub async fn handle_cache_stats<TSys>(
    ctx: &LoadedContext<TSys>,
    req: CacheStatsRequest,
) -> eyre::Result<CacheStats>
where
    TSys: TaskExecutorSys + Clone,
{
    let cache_store = ctx.as_context().create_cache_store();

    let projects: Vec<&str> = req.project.iter().map(String::as_str).collect();
    let tasks: Vec<&str> = req.task.iter().map(String::as_str).collect();
    let dirs: Vec<&str> = req.dir.iter().map(String::as_str).collect();

    let needs_context = !dirs.is_empty() || req.meta.is_some();

    let stats = if needs_context {
        let wrapper = CacheCtxWrapper::new(ctx);
        cache_store
            .get_stats(&CacheStatsArgs {
                project_name_globs: &projects,
                task_name_globs: &tasks,
                dir_globs: &dirs,
                meta_filter: req.meta.as_deref(),
                context: Some(wrapper),
            })
            .await?
    } else {
        cache_store
            .get_stats(&CacheStatsArgs::<()> {
                project_name_globs: &projects,
                task_name_globs: &tasks,
                dir_globs: &[],
                meta_filter: None,
                context: None,
            })
            .await?
    };

    Ok(stats)
}

/// Compute (and optionally delete) prunable cache entries.
///
/// When `req.dry_run == true` the returned entries are not deleted; pass them
/// to [`handle_cache_force_prune`] to actually remove them.
pub async fn handle_cache_prune<TSys>(
    ctx: &LoadedContext<TSys>,
    req: CachePruneRequest,
) -> eyre::Result<CachePruneResponse>
where
    TSys: TaskExecutorSys + Clone,
{
    let cache_store = ctx.as_context().create_cache_store();

    let projects: Vec<&str> = req.project.iter().map(String::as_str).collect();
    let tasks: Vec<&str> = req.task.iter().map(String::as_str).collect();
    let dirs: Vec<&str> = req.dir.iter().map(String::as_str).collect();

    let needs_context =
        req.stale_only || !dirs.is_empty() || req.meta.is_some();

    let entries = if needs_context {
        let wrapper = CacheCtxWrapper::new(ctx);
        cache_store
            .prune_caches(&PruneCacheArgs {
                dry_run: req.dry_run,
                stale_only: req.stale_only,
                older_than: req.older_than,
                project_name_globs: &projects,
                task_name_globs: &tasks,
                dir_globs: &dirs,
                meta_filter: req.meta.as_deref(),
                larger_than: req.larger_than,
                context: Some(wrapper),
            })
            .await?
    } else {
        cache_store
            .prune_caches(&PruneCacheArgs::<()> {
                dry_run: req.dry_run,
                stale_only: false,
                older_than: req.older_than,
                project_name_globs: &projects,
                task_name_globs: &tasks,
                dir_globs: &[],
                meta_filter: None,
                larger_than: req.larger_than,
                context: None,
            })
            .await?
    };

    Ok(CachePruneResponse {
        entries,
        dry_run: req.dry_run,
    })
}

/// Delete the entries returned by a previous [`handle_cache_prune`] call.
pub async fn handle_cache_force_prune<TSys>(
    ctx: &Context<TSys>,
    entries: Vec<PrunedCacheEntry>,
) -> eyre::Result<()>
where
    TSys: TaskExecutorSys + Clone,
{
    let cache_store = ctx.create_cache_store();
    cache_store.force_prune_caches(&entries).await?;
    Ok(())
}

/// Configure a remote cache server.
pub async fn handle_cache_remote_setup<TSys>(
    ctx: &Context<TSys>,
    req: CacheRemoteSetupRequest,
) -> eyre::Result<()>
where
    TSys: TaskExecutorSys + Clone,
{
    let client = ctx.create_remote_cache_client();
    let ext = if req.secure { "enc" } else { "yaml" };
    let config_path = ctx.remote_cache_configuration_path(ext);
    let user = ctx.root_dir().to_string_lossy();

    omni_setup::setup_remote_caching_config_async(
        &user,
        &client,
        config_path.as_path(),
        &req.api_base_url,
        &req.api_key,
        &req.tenant,
        &req.org,
        &req.ws,
        req.env.as_deref(),
        req.secure,
        ctx.sys(),
    )
    .await?;

    Ok(())
}

// ── CacheContext wrapper ──────────────────────────────────────────────────────

/// Adapts [`LoadedContext`] to the [`omni_cache::Context`] trait so that
/// cache stat / prune operations can resolve project/task metadata.
#[derive(new)]
#[repr(transparent)]
struct CacheCtxWrapper<'a, TSys: ContextSys> {
    context: &'a LoadedContext<TSys>,
}

impl<'a, TSys: ContextSys> CacheContext for CacheCtxWrapper<'a, TSys> {
    type Error = LoadedContextError;

    fn get_project_meta_config(
        &self,
        project_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.context.get_project_meta_config(project_name)
    }

    fn get_task_meta_config(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&MetaConfiguration> {
        self.context.get_task_meta_config(project_name, task_name)
    }

    fn get_project_graph(&self) -> Result<ProjectGraph, Self::Error> {
        self.context.get_project_graph()
    }

    fn projects(&self) -> &[Project] {
        self.context.projects()
    }

    fn get_task_env_vars(
        &self,
        node: &TaskExecutionNode,
    ) -> Result<Option<std::sync::Arc<EnvVarsMap>>, Self::Error> {
        self.context.get_task_env_vars(node)
    }

    fn get_cache_info(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&CacheInfo> {
        self.context.get_cache_info(project_name, task_name)
    }

    fn get_output_logs(
        &self,
        project_name: &str,
        task_name: &str,
    ) -> Option<&omni_task_output_logs::OutputLogsConfiguration> {
        self.context.get_output_logs(project_name, task_name)
    }

    fn root_dir(&self) -> &std::path::Path {
        self.context.root_dir()
    }

    fn get_task_override_args(
        &self,
        _project_name: &str,
        _task_name: &str,
    ) -> Option<&UnorderedMap<String, serde_json::Value>> {
        None
    }
}
