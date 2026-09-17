use std::collections::HashSet;

use omni_context::{Context, ContextSys};
use omni_remote_source::{
    InstallOptions, RemoteSource, RemoteSourceContributor,
    manager::{RemoteSourceManager, config::RemoteSourceConfig},
    sys::RemoteSourceSys,
};
use omni_remote_source_contributors::{
    GeneratorRemoteContributor, PackRemoteContributor,
    ProjectionRemoteContributor, ToolRemoteContributor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;
use url::Url;

/// Open the single shared remote-source store for the workspace, rooted at
/// `.omni/sources/`. Every subsystem and `install` resolve remote sources
/// through the manager this returns, so fetch, pin, and deduplication behavior
/// is identical across them.
pub async fn open_source_store<TSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
) -> eyre::Result<RemoteSourceManager<TSys>>
where
    TSys: ContextSys + RemoteSourceSys,
{
    let sources_dir = ctx.omni_dir().join(omni_constants::SOURCES_SEGMENT);
    let store_root = sources_dir.join(omni_constants::STORE_SEGMENT);
    let lockfile_path = sources_dir.join(omni_constants::SOURCE_LOCKFILE_NAME);

    sys.fs_create_dir_all_async(&store_root).await?;

    Ok(RemoteSourceManager::new(
        RemoteSourceConfig::builder()
            .lockfile_path(lockfile_path)
            .store_root_path(store_root)
            .build(),
        sys.clone(),
    )
    .await?)
}

/// Which subsystems an install pass processes. No flag set means all of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SubsystemSelection {
    pub generators: bool,
    pub tools: bool,
    pub projections: bool,
}

impl SubsystemSelection {
    /// Resolve an empty selection to "all subsystems".
    pub fn resolve(self) -> Self {
        if !self.generators && !self.tools && !self.projections {
            Self {
                generators: true,
                tools: true,
                projections: true,
            }
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RemoteSourcesInstallRequest {
    /// Advance mutable refs by re-resolving branches or tags and re-pinning
    /// them.
    pub update: bool,
    /// Which subsystems to process. Empty means all.
    pub select: SubsystemSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsystemReportView {
    pub subsystem: String,
    /// Sources this subsystem was the first to materialize in this run.
    pub materialized: usize,
    /// Sources this subsystem shares with content already materialized by an
    /// earlier subsystem in this run.
    pub deduplicated: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoteSourcesInstallResponse {
    pub subsystems: Vec<SubsystemReportView>,
    /// Store checkouts reclaimed by the union garbage collection.
    pub garbage_collected: usize,
}

/// The system capabilities `omni remote-sources install` needs: workspace
/// context, the shared store's filesystem operations, and the manifest reads
/// the projection walk performs.
pub trait RemoteSourceInstallSys:
    ContextSys + RemoteSourceSys + FsReadAsync
{
}
impl<T: ContextSys + RemoteSourceSys + FsReadAsync> RemoteSourceInstallSys
    for T
{
}

/// Prefetch and pin every remote source declared in the workspace across the
/// selected subsystems, then reconcile the shared store with a union garbage
/// collection. Every subsystem materializes through one shared store, so a
/// repository used by more than one subsystem is fetched once.
pub async fn handle_remote_sources_install<TSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
    req: RemoteSourcesInstallRequest,
) -> eyre::Result<RemoteSourcesInstallResponse>
where
    TSys: RemoteSourceInstallSys,
{
    let select = req.select.resolve();
    let options = InstallOptions { update: req.update };
    let manager = open_source_store(ctx, sys).await?;

    let ws = ctx.workspace_configuration();
    let workspace_root = ctx.root_dir().to_path_buf();

    // Tools materialize last; the order does not affect resolved commits, only
    // which subsystem is credited with first materializing a shared source.
    let mut contributors: Vec<Box<dyn RemoteSourceContributor<TSys>>> =
        Vec::new();
    if select.generators {
        contributors.push(Box::new(GeneratorRemoteContributor::new(
            ws.generators.clone(),
        )));
    }
    if select.projections {
        contributors.push(Box::new(ProjectionRemoteContributor::new(
            ws.projections.clone(),
            workspace_root,
        )));
    }
    if select.tools {
        contributors
            .push(Box::new(ToolRemoteContributor::new(ws.tools.clone())));
    }
    if !ws.packs.is_empty() {
        contributors.push(Box::new(PackRemoteContributor::new(
            ws.packs.clone(),
            ctx.root_dir().to_path_buf(),
        )));
    }

    run_contributors(&manager, contributors, &options).await
}

/// Drive a set of contributors against one shared store: materialize (via each
/// contributor), persist each subsystem's reference set, reconcile with union
/// garbage collection, and aggregate per-subsystem counts. A source shared by
/// two subsystems is credited as materialized to the first and deduplicated to
/// the rest.
async fn run_contributors<TSys>(
    manager: &RemoteSourceManager<TSys>,
    contributors: Vec<Box<dyn RemoteSourceContributor<TSys>>>,
    options: &InstallOptions,
) -> eyre::Result<RemoteSourcesInstallResponse>
where
    TSys: RemoteSourceSys,
{
    let mut subsystems = Vec::with_capacity(contributors.len());
    let mut seen: HashSet<(Url, String)> = HashSet::new();

    for contributor in &contributors {
        let id = contributor.id();
        let refs = contributor.contribute(manager, options).await?;
        manager.record_refs(id, &refs).await?;

        let mut materialized = 0;
        let mut deduplicated = 0;
        for r in &refs {
            let key = match &r.source {
                RemoteSource::Git { uri, .. } => (uri.clone(), r.pin.clone()),
            };
            if seen.insert(key) {
                materialized += 1;
            } else {
                deduplicated += 1;
            }
        }

        subsystems.push(SubsystemReportView {
            subsystem: id.to_string(),
            materialized,
            deduplicated,
        });
    }

    let garbage_collected = manager.retain().await?;

    Ok(RemoteSourcesInstallResponse {
        subsystems,
        garbage_collected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_selection_resolves_to_all_subsystems() {
        let resolved = SubsystemSelection::default().resolve();
        assert!(resolved.generators);
        assert!(resolved.tools);
        assert!(resolved.projections);
    }

    #[test]
    fn a_partial_selection_is_left_untouched() {
        let resolved = SubsystemSelection {
            generators: true,
            ..Default::default()
        }
        .resolve();
        assert!(resolved.generators);
        assert!(!resolved.tools);
        assert!(!resolved.projections);
    }

    #[test]
    fn every_workspace_source_list_has_a_registered_contributor() {
        use omni_remote_source::sys::RemoteSourceSys;
        use system_traits::impls::InMemorySys;

        // Each list on WorkspaceConfiguration must map to a contributor id. If a
        // new source list is added, wire a contributor and extend this guard.
        fn id_of<C: RemoteSourceContributor<InMemorySys>>(c: &C) -> &'static str
        where
            InMemorySys: RemoteSourceSys,
        {
            c.id()
        }

        assert_eq!(
            id_of(&GeneratorRemoteContributor::new(Vec::new())),
            "generator"
        );
        assert_eq!(id_of(&ToolRemoteContributor::new(Vec::new())), "tool");
        assert_eq!(
            id_of(&ProjectionRemoteContributor::new(Vec::new(), ".")),
            "projection"
        );
    }

    struct StubContributor {
        id: &'static str,
        refs: Vec<omni_remote_source::RemoteSourceRef>,
    }

    #[async_trait::async_trait]
    impl<TSys> RemoteSourceContributor<TSys> for StubContributor
    where
        TSys: RemoteSourceSys,
    {
        fn id(&self) -> &'static str {
            self.id
        }

        async fn contribute(
            &self,
            _manager: &RemoteSourceManager<TSys>,
            _options: &InstallOptions,
        ) -> eyre::Result<Vec<omni_remote_source::RemoteSourceRef>> {
            Ok(self.refs.clone())
        }
    }

    fn git_ref(
        uri: &str,
        rev: &str,
        commit: &str,
    ) -> omni_remote_source::RemoteSourceRef {
        omni_remote_source::RemoteSourceRef {
            source: RemoteSource::Git {
                uri: Url::parse(uri).unwrap(),
                rev: rev.to_string(),
            },
            pin: commit.to_string(),
        }
    }

    async fn temp_manager(
        dir: &std::path::Path,
    ) -> RemoteSourceManager<system_traits::impls::RealSys> {
        let sources_root = dir.join(".omni/sources");
        std::fs::create_dir_all(&sources_root).unwrap();
        RemoteSourceManager::new(
            RemoteSourceConfig::builder()
                .lockfile_path(sources_root.join("lock.json"))
                .store_root_path(sources_root.join("store"))
                .build(),
            system_traits::impls::RealSys,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn run_contributors_aggregates_counts_and_deduplicates_shared_sources()
     {
        let dir = tempfile::tempdir().unwrap();
        let manager = temp_manager(dir.path()).await;

        let shared = git_ref("https://example.com/shared.git", "main", "c1");
        let only_a = git_ref("https://example.com/a.git", "main", "c2");

        let contributors: Vec<
            Box<dyn RemoteSourceContributor<system_traits::impls::RealSys>>,
        > = vec![
            Box::new(StubContributor {
                id: "generator",
                refs: vec![shared.clone(), only_a],
            }),
            Box::new(StubContributor {
                id: "tool",
                refs: vec![shared],
            }),
        ];

        let response = run_contributors(
            &manager,
            contributors,
            &InstallOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(response.subsystems.len(), 2);
        assert_eq!(response.subsystems[0].subsystem, "generator");
        assert_eq!(response.subsystems[0].materialized, 2);
        assert_eq!(response.subsystems[0].deduplicated, 0);
        assert_eq!(response.subsystems[1].subsystem, "tool");
        assert_eq!(response.subsystems[1].materialized, 0);
        assert_eq!(response.subsystems[1].deduplicated, 1);

        // Both subsystems' reference sets were persisted.
        let refs_dir = dir.path().join(".omni/sources/refs");
        assert!(refs_dir.join("generator.json").exists());
        assert!(refs_dir.join("tool.json").exists());
    }
}
