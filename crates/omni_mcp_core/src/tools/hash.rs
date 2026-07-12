use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::TaskExecutorSys;

use crate::{
    model::{HashProjectParams, HashResult},
    server::OmniMcpServer,
};

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
    pub(crate) async fn tool_hash_workspace(&self) -> eyre::Result<HashResult> {
        let response = self.make_api().hash_workspace().await?;
        Ok(HashResult {
            hash: response.hash,
        })
    }

    pub(crate) async fn tool_hash_project(
        &self,
        params: HashProjectParams,
    ) -> eyre::Result<HashResult> {
        let response = self
            .make_api()
            .hash_project(&params.name, &params.tasks)
            .await?;
        Ok(HashResult {
            hash: response.hash,
        })
    }
}
