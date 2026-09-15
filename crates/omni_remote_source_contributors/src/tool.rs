use async_trait::async_trait;
use omni_configurations::SourceConfig;
use omni_remote_source::{
    InstallOptions, RemoteSourceContributor, RemoteSourceRef,
    manager::RemoteSourceManager, sys::RemoteSourceSys,
};

/// Materializes the workspace's `tools:` git sources into the shared store.
/// Built from the already-loaded tool source list.
pub struct ToolRemoteContributor {
    sources: Vec<SourceConfig>,
}

impl ToolRemoteContributor {
    pub fn new(sources: Vec<SourceConfig>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl<TSys> RemoteSourceContributor<TSys> for ToolRemoteContributor
where
    TSys: RemoteSourceSys,
{
    fn id(&self) -> &'static str {
        "tool"
    }

    async fn contribute(
        &self,
        manager: &RemoteSourceManager<TSys>,
        options: &InstallOptions,
    ) -> eyre::Result<Vec<RemoteSourceRef>> {
        crate::materialize_flat_git(manager, &self.sources, options).await
    }
}
