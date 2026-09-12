use omni_api::{
    IgnoreCleanRequest, IgnoreCleanResponse, IgnoreSyncRequest,
    IgnoreSyncResponse, IgnoreSys,
};

use crate::{
    model::{IgnoreCleanParams, IgnoreSyncParams},
    server::OmniMcpServer,
};

impl<TSys> OmniMcpServer<TSys>
where
    TSys: omni_context::ContextSys
        + omni_generator::GeneratorSys
        + omni_task_executor::TaskExecutorSys
        + IgnoreSys
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub(crate) async fn tool_ignore_sync(
        &self,
        params: IgnoreSyncParams,
    ) -> eyre::Result<IgnoreSyncResponse> {
        self.make_api()
            .ignore_sync(IgnoreSyncRequest {
                dry_run: params.dry_run,
                check: params.check,
            })
            .await
    }

    pub(crate) async fn tool_ignore_clean(
        &self,
        _params: IgnoreCleanParams,
    ) -> eyre::Result<IgnoreCleanResponse> {
        self.make_api()
            .ignore_clean(IgnoreCleanRequest::default())
            .await
    }
}
