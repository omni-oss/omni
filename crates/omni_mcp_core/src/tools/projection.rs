use omni_api::{
    ProjectionPruneRequest, ProjectionPruneResponse, ProjectionStatusRequest,
    ProjectionStatusResponse, ProjectionSyncRequest, ProjectionSyncResponse,
    ProjectionSys, ProjectionUnlinkRequest, ProjectionUnlinkResponse,
};

use crate::{
    model::{
        ProjectionPruneParams, ProjectionStatusParams, ProjectionSyncParams,
        ProjectionUnlinkParams,
    },
    server::OmniMcpServer,
};

impl<TSys> OmniMcpServer<TSys>
where
    TSys: omni_context::ContextSys
        + omni_generator::GeneratorSys
        + omni_task_executor::TaskExecutorSys
        + ProjectionSys
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub(crate) async fn tool_projection_sync(
        &self,
        params: ProjectionSyncParams,
    ) -> eyre::Result<ProjectionSyncResponse> {
        self.make_api()
            .projection_sync(ProjectionSyncRequest {
                dry_run: params.dry_run,
                force: params.force,
                update: params.update,
                source: params.source,
            })
            .await
    }

    pub(crate) async fn tool_projection_status(
        &self,
        params: ProjectionStatusParams,
    ) -> eyre::Result<ProjectionStatusResponse> {
        self.make_api()
            .projection_status(ProjectionStatusRequest {
                verbose: params.verbose,
            })
            .await
    }

    pub(crate) async fn tool_projection_unlink(
        &self,
        params: ProjectionUnlinkParams,
    ) -> eyre::Result<ProjectionUnlinkResponse> {
        self.make_api()
            .projection_unlink(ProjectionUnlinkRequest {
                id: params.id,
                backup_handling: params.backup_handling,
                clean_backups: params.clean_backups,
            })
            .await
    }

    pub(crate) async fn tool_projection_prune(
        &self,
        params: ProjectionPruneParams,
    ) -> eyre::Result<ProjectionPruneResponse> {
        self.make_api()
            .projection_prune(ProjectionPruneRequest {
                dry_run: params.dry_run,
            })
            .await
    }
}
