use bridge_rpc_services::{FsSys, ProcSys};
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::TaskExecutorSys;
use system_traits::BaseFsMetadataAsync;

use crate::{
    model::{
        ToolInspectParams, ToolInspectResult, ToolListResult, ToolRunParams,
        ToolRunResult, ToolSummary,
    },
    server::OmniMcpServer,
};

impl<TSys> OmniMcpServer<TSys>
where
    TSys: ContextSys
        + GeneratorSys
        + TaskExecutorSys
        + FsSys
        + ProcSys
        + Clone
        + Send
        + Sync
        + 'static,
    <TSys as BaseFsMetadataAsync>::Metadata: Send,
{
    pub(crate) async fn tool_tool_list(&self) -> eyre::Result<ToolListResult> {
        let response = self.make_api().tool_list().await?;
        Ok(ToolListResult {
            tools: response
                .tools
                .into_iter()
                .map(|t| ToolSummary {
                    name: t.name,
                    description: t.description,
                })
                .collect(),
        })
    }

    pub(crate) async fn tool_tool_inspect(
        &self,
        params: ToolInspectParams,
    ) -> eyre::Result<ToolInspectResult> {
        let response = self.make_api().tool_inspect(&params.name).await?;
        Ok(ToolInspectResult {
            name: response.name,
            description: response.description,
            input_schema: response.input_schema,
        })
    }

    pub(crate) async fn tool_tool_run(
        &self,
        params: ToolRunParams,
    ) -> eyre::Result<ToolRunResult> {
        let working_dir = params
            .working_dir
            .map(|dir| omni_api::ToolWorkingDir::Path(dir.into()));
        let result = self
            .make_api()
            .tool_run(&params.name, params.args, working_dir)
            .await?;
        Ok(ToolRunResult { result })
    }
}
