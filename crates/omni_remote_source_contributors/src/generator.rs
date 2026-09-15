use async_trait::async_trait;
use omni_configurations::SourceConfig;
use omni_remote_sources::{
    InstallOptions, RemoteSourceContributor, RemoteSourceRef,
    manager::RemoteSourceManager, sys::RemoteSourceSys,
};

/// Materializes the workspace's `generators:` git sources into the shared
/// store. Built from the already-loaded generator source list.
pub struct GeneratorRemoteContributor {
    sources: Vec<SourceConfig>,
}

impl GeneratorRemoteContributor {
    pub fn new(sources: Vec<SourceConfig>) -> Self {
        Self { sources }
    }
}

#[async_trait]
impl<TSys> RemoteSourceContributor<TSys> for GeneratorRemoteContributor
where
    TSys: RemoteSourceSys,
{
    fn id(&self) -> &'static str {
        "generator"
    }

    async fn contribute(
        &self,
        manager: &RemoteSourceManager<TSys>,
        options: &InstallOptions,
    ) -> eyre::Result<Vec<RemoteSourceRef>> {
        crate::materialize_flat_git(manager, &self.sources, options).await
    }
}
