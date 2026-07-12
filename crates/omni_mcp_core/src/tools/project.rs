use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_task_executor::TaskExecutorSys;

use crate::{
    model::{
        ProjectConfigParams, ProjectConfigResult, ProjectListResult,
        TaskSummary,
    },
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
    pub(crate) async fn tool_project_list(
        &self,
    ) -> eyre::Result<ProjectListResult> {
        let projects = self.make_api().project_list().await?;
        Ok(ProjectListResult { projects })
    }

    pub(crate) async fn tool_project_config(
        &self,
        params: ProjectConfigParams,
    ) -> eyre::Result<ProjectConfigResult> {
        let config = self.make_api().project_config(&params.name).await?;
        let tasks = config
            .tasks
            .iter()
            .map(|(name, _task)| TaskSummary {
                name: name.clone(),
                description: None,
            })
            .collect();
        Ok(ProjectConfigResult {
            name: config.name.clone(),
            dir: config.dir.unresolved_path().to_string_lossy().to_string(),
            description: config
                .description
                .as_ref()
                .map(|d| d.as_ref().clone()),
            tasks,
        })
    }
}
