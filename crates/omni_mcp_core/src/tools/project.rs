use omni_api::{OmniApi, ProjectConfigResponse, ProjectListRequest};
use omni_context::ContextSys;
use omni_generator::GeneratorSys;
use omni_messages::NoopSubscriber;
use omni_task_executor::TaskExecutorSys;

use crate::{
    model::{
        ProjectConfigParams, ProjectConfigResult, ProjectListParams,
        ProjectListResult, TaskSummary,
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
        params: ProjectListParams,
    ) -> eyre::Result<ProjectListResult> {
        let entries = self
            .make_api()?
            .project_list(ProjectListRequest {
                include_base: params.include_base,
                include_extended: params.include_extended,
                all: params.all,
            })
            .await?;
        let projects = entries.iter().map(|entry| entry.name.clone()).collect();
        Ok(ProjectListResult { projects, entries })
    }

    pub(crate) async fn tool_project_config(
        &self,
        params: ProjectConfigParams,
    ) -> eyre::Result<ProjectConfigResult> {
        let ctx = self.context()?;
        let selector = params.into_selector(ctx.root_dir())?;
        let ProjectConfigResponse { config, warnings } =
            OmniApi::new_with_sys(ctx, NoopSubscriber)
                .project_config(selector)
                .await?;
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
            warnings,
        })
    }
}
