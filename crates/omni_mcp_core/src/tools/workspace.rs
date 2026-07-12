use omni_api::EnvRequest;
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::TaskExecutorSys;

use crate::{model::WorkspaceInfoResult, server::OmniMcpServer};

impl<TSys> OmniMcpServer<TSys>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + Clone
        + Send
        + Sync
        + 'static,
{
    pub(crate) async fn tool_workspace_info(
        &self,
    ) -> eyre::Result<WorkspaceInfoResult> {
        let api = self.make_api();
        let cache_dir = api.cache_dir().await.to_string_lossy().to_string();
        let root_dir = self.ctx.root_dir().to_string_lossy().to_string();
        let env = api.get_env(EnvRequest::default()).await?;
        Ok(WorkspaceInfoResult {
            root_dir,
            cache_dir,
            env_vars: env.vars,
        })
    }
}
