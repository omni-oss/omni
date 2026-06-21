use omni_api::{CachePruneRequest, CacheStatsRequest};
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_messages::OmniEventSubscriber;
use omni_task_executor::TaskExecutorSys;

use crate::{
    model::{
        CachePruneParams, CachePruneResult, CacheStatsParams, CacheStatsResult,
        ProjectCacheStatsSummary, TaskCacheStatsSummary,
    },
    server::OmniMcpServer,
};

impl<TSys, S> OmniMcpServer<TSys, S>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
    S: OmniEventSubscriber + Send + Sync + 'static,
{
    pub(crate) async fn tool_cache_stats(
        &self,
        params: CacheStatsParams,
    ) -> eyre::Result<CacheStatsResult> {
        let req = CacheStatsRequest {
            project: params.project,
            task: params.task,
            ..Default::default()
        };
        let stats = self.make_api().cache_stats(req).await?;
        let projects = stats
            .projects
            .into_iter()
            .map(|p| ProjectCacheStatsSummary {
                project_name: p.project_name,
                tasks: p
                    .tasks
                    .into_iter()
                    .map(|t| TaskCacheStatsSummary {
                        task_name: t.task_name,
                        total_size_bytes: t.total_size.as_u64(),
                        cached_files_count: t.cached_files.len(),
                    })
                    .collect(),
            })
            .collect();
        Ok(CacheStatsResult { projects })
    }

    pub(crate) async fn tool_cache_prune(
        &self,
        params: CachePruneParams,
    ) -> eyre::Result<CachePruneResult> {
        let req = CachePruneRequest {
            dry_run: params.dry_run,
            stale_only: params.stale_only,
            project: params.project,
            task: params.task,
            dir: params.dir,
            larger_than: params.larger_than,
            meta: params.meta,
            older_than: params.older_than,
        };
        let api = self.make_api();
        let prune_response = api.cache_prune(req).await?;
        let entries_pruned = prune_response.entries.len();
        let bytes_freed: u64 =
            prune_response.entries.iter().map(|e| e.size.as_u64()).sum();

        if !params.dry_run {
            api.cache_force_prune(prune_response.entries).await?;
        }

        Ok(CachePruneResult {
            dry_run: params.dry_run,
            entries_pruned,
            bytes_freed,
        })
    }
}
