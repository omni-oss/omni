use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use async_trait::async_trait;
use omni_configuration_discovery::ConfigurationDiscovery;
use omni_configurations::{
    OwnedProjectionConfiguration, ProjectionProfile, SourceConfig,
    types::SingleOrMany,
};
use omni_meta::{
    DEFAULT_META_PROJECTION_DEPTH, Materialized, MetaExpand, Node,
    SourceIdentity, expand,
};
use omni_remote_source::{
    InstallOptions, RemoteSource, RemoteSourceContributor, RemoteSourceRef,
    manager::RemoteSourceManager, sys::RemoteSourceSys,
};
use system_traits::FsReadAsync;

/// Materializes every source in the workspace's projection graph, following
/// meta bundles transitively so every git repository reachable from a
/// projection root is fetched. Built from the already-loaded projection source
/// list and the workspace root.
pub struct ProjectionRemoteContributor {
    sources: Vec<SourceConfig<ProjectionProfile>>,
    workspace_root: PathBuf,
}

impl ProjectionRemoteContributor {
    pub fn new(
        sources: Vec<SourceConfig<ProjectionProfile>>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            sources,
            workspace_root: workspace_root.into(),
        }
    }
}

#[async_trait]
impl<TSys> RemoteSourceContributor<TSys> for ProjectionRemoteContributor
where
    TSys: RemoteSourceSys + FsReadAsync + Send + Sync,
{
    fn id(&self) -> &'static str {
        "projection"
    }

    async fn contribute(
        &self,
        manager: &RemoteSourceManager<TSys>,
        options: &InstallOptions,
    ) -> eyre::Result<Vec<RemoteSourceRef>> {
        let expander = MaterializeOnlyExpand {
            sys: manager.sys(),
            manager,
            update: options.update,
            refs: Mutex::new(Vec::new()),
        };

        expand(
            &expander,
            &self.sources,
            &self.workspace_root,
            DEFAULT_META_PROJECTION_DEPTH,
            None,
        )
        .await?;

        Ok(expander.refs.into_inner().expect("refs mutex poisoned"))
    }
}

/// A [`MetaExpand`] that only fetches and pins sources, ignoring routes and
/// trust. It walks the projection graph exactly as `projection sync` does,
/// recording every git reference it resolves.
struct MaterializeOnlyExpand<'a, TSys: RemoteSourceSys> {
    sys: &'a TSys,
    manager: &'a RemoteSourceManager<TSys>,
    update: bool,
    refs: Mutex<Vec<RemoteSourceRef>>,
}

impl<TSys> MetaExpand for MaterializeOnlyExpand<'_, TSys>
where
    TSys: RemoteSourceSys + FsReadAsync + Send + Sync,
{
    type Profile = ProjectionProfile;
    type Leaf = ();
    type Error = eyre::Report;

    async fn classify(
        &self,
        src: &SourceConfig<ProjectionProfile>,
        qualified_id: &str,
        parent_root: &Path,
        depth: usize,
    ) -> eyre::Result<Materialized<(), ProjectionProfile>> {
        let (root, pin, identity) = match src {
            SourceConfig::Local(local) => {
                let path = single_path(qualified_id, &local.path)?;
                let root =
                    resolve_child_root(parent_root, path, depth, qualified_id)?;
                let identity = SourceIdentity::Local(root.clone());
                (root, None, identity)
            }
            SourceConfig::Git(git) => {
                if self.update {
                    self.manager.invalidate_git(&git.uri, &git.rev).await?;
                }

                let remote = RemoteSource::Git {
                    uri: git.uri.clone(),
                    rev: git.rev.clone(),
                };
                let materialized = self.manager.materialize(&remote).await?;
                let pin = materialized.pin.clone();

                self.refs.lock().expect("refs mutex poisoned").push(
                    RemoteSourceRef {
                        source: remote,
                        pin: materialized.pin,
                    },
                );

                let identity = SourceIdentity::Git {
                    uri: git.uri.clone(),
                    rev: git.rev.clone(),
                };
                (materialized.root, Some(pin), identity)
            }
            SourceConfig::Registry(_) => {
                unreachable!("registry sources are never constructed")
            }
        };

        let manifest = discover_owned_manifest(self.sys, &root).await?;
        let node = match manifest {
            Some(OwnedProjectionConfiguration::Meta { sources }) => {
                Node::meta(sources)
            }
            _ => Node::leaf(()),
        };

        Ok(Materialized {
            node,
            identity,
            root,
            pin,
        })
    }

    fn declared_id<'a>(
        &self,
        src: &'a SourceConfig<ProjectionProfile>,
    ) -> &'a str {
        match src {
            SourceConfig::Local(local) => local.extra.id.as_str(),
            SourceConfig::Git(git) => git.extra.id.as_str(),
            SourceConfig::Registry(registry) => {
                registry.extra.id.as_deref().unwrap_or_default()
            }
        }
    }
}

fn single_path<'a>(
    id: &str,
    path: &'a SingleOrMany<String>,
) -> eyre::Result<&'a str> {
    match path {
        SingleOrMany::Single(p) => Ok(p.as_str()),
        SingleOrMany::Many(items) if items.len() == 1 => Ok(items[0].as_str()),
        SingleOrMany::Many(_) => Err(eyre::eyre!(
            "projection source '{id}' declares multiple paths; a projection source takes exactly one path"
        )),
    }
}

fn resolve_child_root(
    parent_root: &Path,
    path: &str,
    depth: usize,
    id: &str,
) -> eyre::Result<PathBuf> {
    let root = path_clean::clean(parent_root.join(path));
    if depth > 0 {
        let base = path_clean::clean(parent_root);
        if !root.starts_with(&base) {
            return Err(eyre::eyre!(
                "bundled projection source '{id}' resolves to '{}', outside its parent '{}'",
                root.display(),
                base.display()
            ));
        }
    }
    Ok(root)
}

async fn discover_owned_manifest<TSys>(
    sys: &TSys,
    source_root: &Path,
) -> eyre::Result<Option<OwnedProjectionConfiguration>>
where
    TSys: FsReadAsync + Send + Sync,
{
    static NAMES: std::sync::LazyLock<Vec<String>> =
        std::sync::LazyLock::new(|| {
            omni_constants::config_file_names(omni_constants::PROJECTION_OMNI)
        });
    const IGNORE_FILES: [&str; 1] = [omni_constants::OMNI_IGNORE];

    let no_exclude: &[String] = &[];
    let discovery = ConfigurationDiscovery::new(
        source_root,
        &NAMES[..],
        no_exclude,
        &NAMES[..],
        &IGNORE_FILES[..],
        "projection",
    );

    for file in discovery.discover().await? {
        let owned: OwnedProjectionConfiguration =
            omni_file_data_serde::read_async(file.as_path(), sys).await?;
        return Ok(Some(owned));
    }

    Ok(None)
}
