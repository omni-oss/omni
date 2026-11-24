use std::{collections::HashSet, path::PathBuf, time::Duration};

use bytesize::ByteSize;
use derive_new::new;
use maps::UnorderedMap;
use omni_cache::{
    CacheStats, CacheStatsArgs, Context as CacheContext, PruneCacheArgs,
    PruneInventory, PrunedCacheEntry, TaskExecutionCacheStore,
    impls::{HybridTaskExecutionCacheStore, RemoteConfig},
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
    /// Exact project names; glob characters are literal.
    pub project: Vec<String>,
    /// Project name globs, in addition to `project`.
    pub project_pattern: Vec<String>,
    /// Exact task names; glob characters are literal.
    pub task: Vec<String>,
    /// Task name globs, in addition to `task`.
    pub task_pattern: Vec<String>,
    pub dir: Vec<String>,
    pub meta: Option<String>,
}

/// Filters for [`handle_cache_prune`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct CachePruneRequest {
    /// Only prune cache entries whose digest is stale.
    #[serde(alias = "stale_only")]
    pub stale: bool,
    /// Only prune entries for tasks absent from every cache-sharing worktree.
    #[serde(default)]
    pub orphan: bool,
    /// Only prune entries older than this duration.
    pub older_than: Option<Duration>,
    /// Only prune entries larger than this byte size.
    #[schemars(with = "Option<u64>")]
    pub larger_than: Option<ByteSize>,
    /// Exact project names; glob characters are literal.
    pub project: Vec<String>,
    /// Project name globs, in addition to `project`.
    pub project_pattern: Vec<String>,
    /// Exact task names; glob characters are literal.
    pub task: Vec<String>,
    /// Task name globs, in addition to `task`.
    pub task_pattern: Vec<String>,
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
    /// Upload successful results (off by default).
    #[serde(default)]
    pub push: bool,
    /// Where uploads go.
    #[serde(default)]
    pub target: omni_configurations::RemoteTarget,
    /// The isolated scope label: `auto`, `run` or a literal.
    #[serde(default)]
    pub scope: omni_configurations::ScopeConfig,
    /// The size cap in bytes for uploads and downloads.
    #[serde(default)]
    pub max_artifact_size: Option<u64>,
    /// The publisher's signing key reference (never the key).
    #[serde(default)]
    pub signing_key: Option<omni_configurations::SigningKeyConfiguration>,
    /// A local verify mode; it can only tighten the workspace's.
    #[serde(default)]
    pub verify: Option<omni_configurations::VerifyMode>,
    /// Narrow the workspace's trusted keys to these ids.
    #[serde(default)]
    pub trusted_keys: Option<Vec<String>>,
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
    /// Advisory alias suggestions for exact task selectors that matched no
    /// stored name. They never change the selection, the approved paths or a
    /// confirmation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<CacheAliasHint>,
}

/// The result of [`handle_cache_stats`]: the stored statistics, plus advisory
/// alias hints.
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheStatsResponse {
    #[serde(flatten)]
    pub stats: CacheStats,
    /// Advisory alias suggestions for exact task selectors that matched no
    /// stored name. They never change which entries are listed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<CacheAliasHint>,
}

/// A live alias that names the same task as an exact selector which matched no
/// stored cache entry. Stored names are canonical, so the suggestion is to
/// select the canonical name; nothing is selected on the caller's behalf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CacheAliasHint {
    /// The exact task selector that matched no stored entry.
    pub selector: String,
    /// The project that declares the alias.
    pub project: String,
    /// The alias; equal to `selector`.
    pub alias: String,
    /// The canonical task name to select instead, with `project`.
    pub canonical: String,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// Show per-project cache statistics.
pub async fn handle_cache_stats<TSys>(
    ctx: &LoadedContext<TSys>,
    req: CacheStatsRequest,
) -> eyre::Result<CacheStatsResponse>
where
    TSys: TaskExecutorSys + Clone,
{
    let cache_store = ctx.as_context().create_cache_store();

    let projects: Vec<&str> = req.project.iter().map(String::as_str).collect();
    let project_patterns: Vec<&str> =
        req.project_pattern.iter().map(String::as_str).collect();
    let tasks: Vec<&str> = req.task.iter().map(String::as_str).collect();
    let task_patterns: Vec<&str> =
        req.task_pattern.iter().map(String::as_str).collect();
    let dirs: Vec<&str> = req.dir.iter().map(String::as_str).collect();

    let needs_context = !dirs.is_empty() || req.meta.is_some();

    let stats = if needs_context {
        let wrapper = CacheCtxWrapper::new(ctx);
        cache_store
            .get_stats(&CacheStatsArgs {
                project_names: &projects,
                project_name_globs: &project_patterns,
                task_names: &tasks,
                task_name_globs: &task_patterns,
                dir_globs: &dirs,
                meta_filter: req.meta.as_deref(),
                context: Some(wrapper),
            })
            .await?
    } else {
        cache_store
            .get_stats(&CacheStatsArgs::<()> {
                project_names: &projects,
                project_name_globs: &project_patterns,
                task_names: &tasks,
                task_name_globs: &task_patterns,
                dir_globs: &[],
                meta_filter: None,
                context: None,
            })
            .await?
    };

    let hints = alias_hints(
        ctx,
        &cache_store,
        &req.project,
        &req.project_pattern,
        &req.task,
    )
    .await;

    Ok(CacheStatsResponse { stats, hints })
}

/// Suggest the canonical name for each exact task selector that matches no
/// stored `(project, task)` pair in its project scope but names a live alias.
///
/// Best effort and advisory: it reads the invoking workspace's already loaded
/// definitions (no new load, no environment evaluation, never a peer
/// workspace), decides a miss on the stored cache before any directory, meta,
/// age, size, stale or orphan predicate, and drops every hint if the stored
/// names cannot be read. Patterns get no hint.
async fn alias_hints<TSys>(
    ctx: &LoadedContext<TSys>,
    cache_store: &impl TaskExecutionCacheStore,
    projects: &[String],
    project_patterns: &[String],
    tasks: &[String],
) -> Vec<CacheAliasHint>
where
    TSys: TaskExecutorSys + Clone,
{
    use omni_execution_plan::{
        DefaultProjectFilter, ProjectFilterExt as _, ProjectSelector,
    };

    if tasks.is_empty() || ctx.projects().iter().all(|p| p.aliases.is_empty()) {
        return vec![];
    }
    let selectors = projects
        .iter()
        .map(ProjectSelector::exact)
        .chain(project_patterns.iter().map(ProjectSelector::pattern))
        .collect::<Vec<_>>();
    let Ok(filter) = DefaultProjectFilter::new(&selectors) else {
        return vec![];
    };
    let scoped = filter.filter_projects(ctx.projects());
    let exact_projects: Vec<&str> =
        projects.iter().map(String::as_str).collect();
    let project_globs: Vec<&str> =
        project_patterns.iter().map(String::as_str).collect();

    let mut hints = vec![];
    let mut seen = HashSet::new();
    for task in tasks {
        if !seen.insert(task.as_str()) {
            continue;
        }
        let candidates = scoped
            .iter()
            .filter(|p| p.is_alias(task))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }
        let stored = cache_store
            .get_stats(&CacheStatsArgs::<()> {
                project_names: &exact_projects,
                project_name_globs: &project_globs,
                task_names: &[task.as_str()],
                task_name_globs: &[],
                dir_globs: &[],
                meta_filter: None,
                context: None,
            })
            .await;
        let Ok(stored) = stored else { return vec![] };
        if stored.projects.iter().any(|p| !p.tasks.is_empty()) {
            continue;
        }
        for project in candidates {
            if let Some(canonical) = project.aliases.resolve(task) {
                hints.push(CacheAliasHint {
                    selector: task.clone(),
                    project: project.name.clone(),
                    alias: task.clone(),
                    canonical: canonical.to_owned(),
                });
            }
        }
    }
    hints
}

/// Compute (and optionally delete) prunable cache entries.
///
/// When `req.dry_run == true` no entries are deleted. Use
/// [`crate::OmniApi::cache_confirm_prune`] to recheck a preview before deletion.
pub async fn handle_cache_prune<TSys>(
    ctx: &LoadedContext<TSys>,
    req: CachePruneRequest,
) -> eyre::Result<CachePruneResponse>
where
    TSys: TaskExecutorSys + Clone,
{
    if req.orphan || req.stale {
        handle_cache_prune_unloaded(ctx.as_context(), req, None).await
    } else {
        handle_cache_prune_inner(ctx, req, None).await
    }
}

pub(crate) async fn handle_cache_confirm_prune<TSys>(
    ctx: &Context<TSys>,
    mut req: CachePruneRequest,
    entries: Vec<PrunedCacheEntry>,
) -> eyre::Result<CachePruneResponse>
where
    TSys: TaskExecutorSys + Clone,
{
    req.dry_run = false;
    let approved = entries.into_iter().map(|entry| entry.entry_dir).collect();
    handle_cache_prune_unloaded(ctx, req, Some(&approved)).await
}

pub(crate) async fn handle_cache_prune_unloaded<TSys>(
    ctx: &Context<TSys>,
    req: CachePruneRequest,
    approved: Option<&HashSet<PathBuf>>,
) -> eyre::Result<CachePruneResponse>
where
    TSys: TaskExecutorSys + Clone,
{
    if req.orphan && (req.stale || !req.dir.is_empty() || req.meta.is_some()) {
        eyre::bail!(
            "orphan pruning cannot be combined with stale, directory, or metadata filters"
        );
    }
    // Load a fresh, non-executing view even if the API already held a loaded
    // context from the preview or another command.
    let ctx = if req.orphan || req.stale {
        ctx.reload()?.into_loaded_for_cache_prune(req.stale).await?
    } else {
        ctx.reload()?.into_loaded().await?
    };
    handle_cache_prune_inner(&ctx, req, approved).await
}

async fn handle_cache_prune_inner<TSys>(
    ctx: &LoadedContext<TSys>,
    req: CachePruneRequest,
    approved: Option<&HashSet<PathBuf>>,
) -> eyre::Result<CachePruneResponse>
where
    TSys: TaskExecutorSys + Clone,
{
    let cache_store = ctx.as_context().create_cache_store();

    let projects: Vec<&str> = req.project.iter().map(String::as_str).collect();
    let project_patterns: Vec<&str> =
        req.project_pattern.iter().map(String::as_str).collect();
    let tasks: Vec<&str> = req.task.iter().map(String::as_str).collect();
    let task_patterns: Vec<&str> =
        req.task_pattern.iter().map(String::as_str).collect();
    let dirs: Vec<&str> = req.dir.iter().map(String::as_str).collect();

    let mut args = PruneCacheArgs {
        dry_run: req.dry_run,
        stale: req.stale,
        orphan: req.orphan,
        inventory: None,
        approved,
        older_than: req.older_than,
        project_names: &projects,
        project_name_globs: &project_patterns,
        task_names: &tasks,
        task_name_globs: &task_patterns,
        dir_globs: &dirs,
        meta_filter: req.meta.as_deref(),
        larger_than: req.larger_than,
        context: Some(CacheCtxWrapper::new(ctx)),
    };
    let mut inventory = PruneInventory::default();
    if req.orphan || req.stale {
        let peers = ctx
            .as_context()
            .load_cache_peer_workspaces(req.stale)
            .await?;
        for workspace in std::iter::once(ctx).chain(&peers) {
            // Hashing needs the workspace root and hash index, but must not
            // probe the remote or require access to a remote partition.
            let hash_store = HybridTaskExecutionCacheStore::new(
                workspace.as_context().cache_dir(),
                workspace.root_dir(),
                RemoteConfig::new_disabled(),
            );
            hash_store
                .observe_prune_workspace(
                    &mut inventory,
                    &CacheCtxWrapper::new(workspace),
                    &args,
                )
                .await?;
        }
        args.inventory = Some(&inventory);
    }
    let entries = cache_store.prune_caches(&args).await?;
    let hints = alias_hints(
        ctx,
        &cache_store,
        &req.project,
        &req.project_pattern,
        &req.task,
    )
    .await;

    Ok(CachePruneResponse {
        entries,
        dry_run: req.dry_run,
        hints,
    })
}

/// Delete entries without rechecking their selection criteria.
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
    let cache_dir = ctx.cache_dir();
    let user = cache_dir.to_string_lossy();

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
        omni_configurations::RemoteCacheSettings {
            push: req.push,
            target: req.target,
            scope: req.scope,
            max_artifact_size: req.max_artifact_size,
            signing_key: req.signing_key,
            verify: req.verify,
            trusted_keys: req.trusted_keys,
        },
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

#[cfg(test)]
mod hint_tests {
    use super::*;

    fn hint() -> CacheAliasHint {
        CacheAliasHint {
            selector: "compile".into(),
            project: "web".into(),
            alias: "compile".into(),
            canonical: "build-native".into(),
        }
    }

    #[test]
    fn stats_json_flattens_the_stats_and_adds_hints_only_when_present() {
        let plain = serde_json::to_value(CacheStatsResponse {
            stats: CacheStats::default(),
            hints: vec![],
        })
        .unwrap();
        assert_eq!(plain, serde_json::json!({ "projects": [] }));
        let hinted = serde_json::to_value(CacheStatsResponse {
            stats: CacheStats::default(),
            hints: vec![hint()],
        })
        .unwrap();
        assert_eq!(hinted["projects"], serde_json::json!([]));
        assert_eq!(hinted["hints"][0]["canonical"], "build-native");
        let back: CacheStatsResponse = serde_json::from_value(plain).unwrap();
        assert!(back.hints.is_empty());
    }

    #[test]
    fn prune_json_omits_empty_hints() {
        let plain = serde_json::to_value(CachePruneResponse {
            entries: vec![],
            dry_run: true,
            hints: vec![],
        })
        .unwrap();
        assert!(plain.get("hints").is_none());
        let old: CachePruneResponse =
            serde_json::from_value(serde_json::json!({
                "entries": [], "dry_run": false
            }))
            .unwrap();
        assert!(old.hints.is_empty());
    }
}
