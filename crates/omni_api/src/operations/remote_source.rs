use std::{collections::HashSet, time::Duration};

use bytesize::ByteSize;

use omni_context::{Context, ContextSys};
use omni_remote_source::{
    InstallOptions, RemoteSourceContributor,
    manager::{
        RemoteSourceManager, StorePruneFilter, config::RemoteSourceConfig,
    },
    sys::RemoteSourceSys,
};
use omni_remote_source_contributors::{
    GeneratorRemoteContributor, PackRemoteContributor,
    ProjectionRemoteContributor, ToolRemoteContributor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;

/// Open the single shared remote-source store for the workspace. The store
/// root is the context's resolved sources directory (a per-user location by
/// default); the lockfile and reference sets stay per workspace under
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
    let store_root = ctx.sources_dir()?.to_path_buf();
    let lockfile_path = sources_dir.join(omni_constants::SOURCE_LOCKFILE_NAME);

    Ok(RemoteSourceManager::new(
        RemoteSourceConfig::builder()
            .lockfile_path(lockfile_path)
            .store_root_path(store_root)
            .build(),
        sys.clone(),
    )
    .await?)
}

/// Filters for [`handle_remote_sources_prune`]. At least one of `all`,
/// `older_than` and `larger_than` is required; an entry must satisfy every
/// filter that is set.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RemoteSourcesPruneRequest {
    /// Select every checkout in the store.
    pub all: bool,
    /// Only checkouts last used longer ago than this.
    pub older_than: Option<Duration>,
    /// Only checkouts larger than this.
    #[schemars(with = "Option<u64>")]
    pub larger_than: Option<ByteSize>,
    /// Only report what would be deleted.
    pub dry_run: bool,
    /// Restrict the prune to these checkouts (as returned by an earlier dry
    /// run). The CLI passes what the user confirmed so a checkout another
    /// workspace adds meanwhile is never deleted unseen.
    #[serde(default)]
    pub only: Option<Vec<StoreCheckoutId>>,
}

/// Identity of one store checkout.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct StoreCheckoutId {
    /// The source kind, e.g. `git`.
    pub kind: String,
    pub remote: String,
    pub pin: String,
}

/// One store checkout that matched a prune.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StoreCheckoutView {
    /// The source kind, e.g. `git`.
    pub kind: String,
    /// The remote's directory name in the store.
    pub remote: String,
    /// The immutable content id (git: the commit).
    pub pin: String,
    pub size_bytes: u64,
    /// Seconds since the checkout was last used.
    pub unused_for_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RemoteSourcesPruneResponse {
    /// The checkouts that matched; deleted unless the request was a dry run.
    pub checkouts: Vec<StoreCheckoutView>,
    pub deleted: bool,
}

/// Delete checkouts from the shared remote-source store. This is the only
/// operation that does: install never deletes, because the store may be shared
/// by many workspaces. A deleted checkout is refetched the next time it is
/// used.
pub async fn handle_remote_sources_prune<TSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
    req: RemoteSourcesPruneRequest,
) -> eyre::Result<RemoteSourcesPruneResponse>
where
    TSys: ContextSys + RemoteSourceSys,
{
    let only: Option<Vec<(String, String, String)>> =
        req.only.as_ref().map(|ids| {
            ids.iter()
                .map(|id| (id.kind.clone(), id.remote.clone(), id.pin.clone()))
                .collect()
        });
    let filter = StorePruneFilter {
        all: req.all,
        older_than: req.older_than,
        larger_than: req.larger_than.map(|size| size.as_u64()),
        only: only.as_deref(),
    };
    if !filter.has_criteria() {
        eyre::bail!(
            "refusing to prune without a filter: pass --older-than, \
             --larger-than or --all"
        );
    }

    let manager = open_source_store(ctx, sys).await?;
    let now = std::time::SystemTime::now();
    let matched = manager.prune_store(&filter, req.dry_run).await?;
    Ok(RemoteSourcesPruneResponse {
        checkouts: matched
            .into_iter()
            .map(|entry| StoreCheckoutView {
                kind: entry.kind,
                remote: entry.remote,
                pin: entry.pin,
                size_bytes: entry.size,
                unused_for_secs: now
                    .duration_since(entry.last_used)
                    .unwrap_or_default()
                    .as_secs(),
            })
            .collect(),
        deleted: !req.dry_run,
    })
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
/// selected subsystems, then drop lockfile pins no subsystem references (the
/// store itself is never pruned here). Every subsystem materializes through one shared store, so a
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
/// contributor), persist each subsystem's reference set, reconcile lockfile
/// pins, and aggregate per-subsystem counts. A source shared by
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
    let mut seen: HashSet<(&'static str, String, String)> = HashSet::new();

    for contributor in &contributors {
        let id = contributor.id();
        let refs = contributor.contribute(manager, options).await?;
        manager.record_refs(id, &refs).await?;

        let mut materialized = 0;
        let mut deduplicated = 0;
        for r in &refs {
            let key = (r.source.kind(), r.source.spec().locator, r.pin.clone());
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

    manager.reconcile_pins().await?;

    Ok(RemoteSourcesInstallResponse { subsystems })
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
            source: omni_remote_source::RemoteSource::Git {
                uri: url::Url::parse(uri).unwrap(),
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
