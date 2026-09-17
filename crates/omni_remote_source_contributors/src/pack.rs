use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use async_trait::async_trait;
use omni_configurations::{
    GeneratorSourceConfiguration, GitSource, LocalSource, PackManifest,
    PackProfile, PackSourceConfiguration, PackSubsystem, ProjectionId,
    ProjectionSourceConfiguration, SourceConfig, ToolSourceConfiguration,
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

/// The payload a pack node contributes: its materialized root, author metadata,
/// the consumer-side `provides` gate declared on the entry that referenced it,
/// and the three subsystem source lists from its manifest.
struct PackLeaf {
    root: PathBuf,
    name: String,
    version: Option<String>,
    description: Option<String>,
    provides: Option<Vec<PackSubsystem>>,
    generators: Vec<GeneratorSourceConfiguration>,
    tools: Vec<ToolSourceConfiguration>,
    projections: Vec<ProjectionSourceConfiguration>,
}

/// One fully resolved pack node in the expanded graph.
#[derive(Debug, Clone)]
pub struct ExpandedPackNode {
    pub id: String,
    pub qualified_id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub root: PathBuf,
    pub pin: Option<String>,
    pub effective_provides: Option<Vec<PackSubsystem>>,
    pub generators: Vec<GeneratorSourceConfiguration>,
    pub tools: Vec<ToolSourceConfiguration>,
    pub projections: Vec<ProjectionSourceConfiguration>,
}

impl ExpandedPackNode {
    fn contributes(&self, subsystem: PackSubsystem) -> bool {
        self.effective_provides
            .as_ref()
            .is_none_or(|set| set.contains(&subsystem))
    }
}

/// A generator/tool source contributed by a pack, carrying the qualified id its
/// discovered item names must be namespaced by and the pack root its `local`
/// paths resolve against.
#[derive(Debug, Clone)]
pub struct PackContributedSource<P: omni_configurations::SourceConfigProfile> {
    pub qualified_id: String,
    pub root: PathBuf,
    pub source: SourceConfig<P>,
}

/// The fully expanded pack graph and the effective subsystem contributions
/// derived from it.
#[derive(Debug, Clone, Default)]
pub struct ExpandedPacks {
    pub nodes: Vec<ExpandedPackNode>,
}

impl ExpandedPacks {
    /// Pack-contributed generator sources, each tagged with the qualified id its
    /// item names must be namespaced by. Gated on `provides` and on the node's
    /// manifest declaring `generators`.
    pub fn effective_generator_sources(
        &self,
    ) -> Vec<PackContributedSource<()>> {
        self.contributed(PackSubsystem::Generators, |n| &n.generators)
    }

    /// Pack-contributed tool sources, tagged and gated like generators.
    pub fn effective_tool_sources(&self) -> Vec<PackContributedSource<()>> {
        self.contributed(PackSubsystem::Tools, |n| &n.tools)
    }

    fn contributed(
        &self,
        subsystem: PackSubsystem,
        pick: impl Fn(&ExpandedPackNode) -> &Vec<SourceConfig<()>>,
    ) -> Vec<PackContributedSource<()>> {
        let mut out = Vec::new();
        for node in &self.nodes {
            if !node.contributes(subsystem) {
                continue;
            }
            for source in pick(node) {
                out.push(PackContributedSource {
                    qualified_id: node.qualified_id.clone(),
                    root: node.root.clone(),
                    source: source.clone(),
                });
            }
        }
        out
    }

    /// Pack-contributed projection sources, ready to feed into the workspace
    /// projection expansion: `local` paths rebased onto the (already
    /// materialized) pack root and each `id` prefixed with the pack's qualified
    /// id. Gated on `provides` and on the node declaring `projections`.
    pub fn effective_projection_sources(
        &self,
    ) -> Vec<ProjectionSourceConfiguration> {
        let mut out = Vec::new();
        for node in &self.nodes {
            if !node.contributes(PackSubsystem::Projections) {
                continue;
            }
            let prefix = node.qualified_id.replace("::", "/");
            for source in &node.projections {
                out.push(rebase_projection_source(source, &node.root, &prefix));
            }
        }
        out
    }
}

/// Prefix a pack projection `id` with the pack's path-safe qualified id and
/// rebase a `local` path onto the materialized pack root.
fn rebase_projection_source(
    source: &ProjectionSourceConfiguration,
    root: &Path,
    prefix: &str,
) -> ProjectionSourceConfiguration {
    match source {
        SourceConfig::Local(local) => {
            let rebased = match &local.path {
                SingleOrMany::Single(p) => {
                    SingleOrMany::Single(rebase_path(root, p))
                }
                SingleOrMany::Many(ps) => SingleOrMany::Many(
                    ps.iter().map(|p| rebase_path(root, p)).collect(),
                ),
            };
            SourceConfig::Local(LocalSource {
                path: rebased,
                base: local.base.clone(),
                extra: ProjectionId {
                    id: format!("{prefix}/{}", local.extra.id),
                },
            })
        }
        SourceConfig::Git(git) => SourceConfig::Git(GitSource {
            uri: git.uri.clone(),
            rev: git.rev.clone(),
            base: git.base.clone(),
            extra: ProjectionId {
                id: format!("{prefix}/{}", git.extra.id),
            },
        }),
        SourceConfig::Registry(_) => {
            unreachable!("registry sources are never constructed")
        }
    }
}

fn rebase_path(root: &Path, path: &str) -> String {
    path_clean::clean(root.join(path))
        .to_string_lossy()
        .into_owned()
}

/// Expand a workspace's `packs:` list into its fully resolved node graph,
/// following sub-packs transitively through the shared meta driver. Returns the
/// expanded packs and every git ref materialized along the way.
pub async fn expand_packs<TSys>(
    manager: &RemoteSourceManager<TSys>,
    sources: &[PackSourceConfiguration],
    workspace_root: &Path,
    update: bool,
) -> eyre::Result<(ExpandedPacks, Vec<RemoteSourceRef>)>
where
    TSys: RemoteSourceSys + FsReadAsync + Send + Sync,
{
    let expander = PackMetaExpand {
        sys: manager.sys(),
        manager,
        update,
        refs: Mutex::new(Vec::new()),
    };

    let effective = expand(
        &expander,
        sources,
        workspace_root,
        DEFAULT_META_PROJECTION_DEPTH,
        None,
    )
    .await?;

    let provides_by_qid: HashMap<&str, &Option<Vec<PackSubsystem>>> = effective
        .iter()
        .map(|e| (e.qualified_id.as_str(), &e.leaf.provides))
        .collect();

    let nodes = effective
        .iter()
        .map(|e| {
            let effective_provides =
                effective_provides(&e.qualified_id, &provides_by_qid);
            ExpandedPackNode {
                id: e.id.clone(),
                qualified_id: e.qualified_id.clone(),
                name: e.leaf.name.clone(),
                version: e.leaf.version.clone(),
                description: e.leaf.description.clone(),
                root: e.leaf.root.clone(),
                pin: e.pin.clone(),
                effective_provides,
                generators: e.leaf.generators.clone(),
                tools: e.leaf.tools.clone(),
                projections: e.leaf.projections.clone(),
            }
        })
        .collect();

    let refs = expander.refs.into_inner().expect("refs mutex poisoned");
    Ok((ExpandedPacks { nodes }, refs))
}

/// The effective `provides` for a node is the intersection of the gate declared
/// at every ancestor along its qualified-id path. `None` at every level means
/// "all subsystems"; any `Some` narrows and the narrowing cascades downward.
fn effective_provides(
    qualified_id: &str,
    provides_by_qid: &HashMap<&str, &Option<Vec<PackSubsystem>>>,
) -> Option<Vec<PackSubsystem>> {
    let mut acc: Option<Vec<PackSubsystem>> = None;
    let mut prefix = String::new();
    for segment in qualified_id.split("::") {
        if prefix.is_empty() {
            prefix.push_str(segment);
        } else {
            prefix.push_str("::");
            prefix.push_str(segment);
        }
        if let Some(provides) = provides_by_qid.get(prefix.as_str()) {
            acc = intersect_provides(acc, provides);
        }
    }
    acc
}

fn intersect_provides(
    acc: Option<Vec<PackSubsystem>>,
    next: &Option<Vec<PackSubsystem>>,
) -> Option<Vec<PackSubsystem>> {
    match (acc, next) {
        (None, None) => None,
        (Some(acc), None) => Some(acc),
        (None, Some(next)) => Some(next.clone()),
        (Some(acc), Some(next)) => {
            Some(acc.into_iter().filter(|s| next.contains(s)).collect())
        }
    }
}

struct PackMetaExpand<'a, TSys: RemoteSourceSys> {
    sys: &'a TSys,
    manager: &'a RemoteSourceManager<TSys>,
    update: bool,
    refs: Mutex<Vec<RemoteSourceRef>>,
}

impl<TSys> MetaExpand for PackMetaExpand<'_, TSys>
where
    TSys: RemoteSourceSys + FsReadAsync + Send + Sync,
{
    type Profile = PackProfile;
    type Leaf = PackLeaf;
    type Error = eyre::Report;

    async fn classify(
        &self,
        src: &SourceConfig<PackProfile>,
        qualified_id: &str,
        parent_root: &Path,
        depth: usize,
    ) -> eyre::Result<Materialized<PackLeaf, PackProfile>> {
        let provides = src.base().provides.clone();

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

        let manifest = discover_pack_manifest(self.sys, &root).await?.ok_or_else(
            || {
                eyre::eyre!(
                    "pack source '{qualified_id}' has no pack.omni.* manifest at its root"
                )
            },
        )?;

        reject_escaping_paths(qualified_id, &manifest)?;

        let children = manifest.packs;
        let leaf = PackLeaf {
            root: root.clone(),
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            provides,
            generators: manifest.generators,
            tools: manifest.tools,
            projections: manifest.projections,
        };

        let has_own_sources = !leaf.generators.is_empty()
            || !leaf.tools.is_empty()
            || !leaf.projections.is_empty();

        let node = match (has_own_sources, children.is_empty()) {
            (true, true) => Node::leaf(leaf),
            (true, false) => Node::both(leaf, children),
            (false, false) => {
                // Composes only sub-packs; still record the node for the tree.
                Node::both(leaf, children)
            }
            (false, true) => Node::leaf(leaf),
        };

        Ok(Materialized {
            node,
            identity,
            root,
            pin,
        })
    }

    fn declared_id<'a>(&self, src: &'a SourceConfig<PackProfile>) -> &'a str {
        match src {
            SourceConfig::Local(local) => local.extra.id.as_str(),
            SourceConfig::Git(git) => git.extra.id.as_str(),
            SourceConfig::Registry(registry) => {
                registry.extra.id.as_deref().unwrap_or_default()
            }
        }
    }
}

fn reject_escaping_paths(
    qualified_id: &str,
    manifest: &PackManifest,
) -> eyre::Result<()> {
    for source in &manifest.generators {
        reject_escaping_source(qualified_id, local_paths(source))?;
    }
    for source in &manifest.tools {
        reject_escaping_source(qualified_id, local_paths(source))?;
    }
    for source in &manifest.projections {
        reject_escaping_source(qualified_id, projection_local_paths(source))?;
    }
    Ok(())
}

fn local_paths(source: &SourceConfig<()>) -> &SingleOrMany<String> {
    match source {
        SourceConfig::Local(local) => &local.path,
        SourceConfig::Git(_) | SourceConfig::Registry(_) => EMPTY_PATHS,
    }
}

fn projection_local_paths(
    source: &ProjectionSourceConfiguration,
) -> &SingleOrMany<String> {
    match source {
        SourceConfig::Local(local) => &local.path,
        SourceConfig::Git(_) | SourceConfig::Registry(_) => EMPTY_PATHS,
    }
}

static EMPTY_PATHS: &SingleOrMany<String> = &SingleOrMany::Many(Vec::new());

fn reject_escaping_source(
    qualified_id: &str,
    paths: &SingleOrMany<String>,
) -> eyre::Result<()> {
    let iter: Box<dyn Iterator<Item = &String>> = match paths {
        SingleOrMany::Single(p) => Box::new(std::iter::once(p)),
        SingleOrMany::Many(ps) => Box::new(ps.iter()),
    };
    for path in iter {
        let p = Path::new(path);
        if p.is_absolute()
            || p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(eyre::eyre!(
                "pack source '{qualified_id}' declares a path that escapes its root: {path}"
            ));
        }
    }
    Ok(())
}

fn single_path<'a>(
    id: &str,
    path: &'a SingleOrMany<String>,
) -> eyre::Result<&'a str> {
    match path {
        SingleOrMany::Single(p) => Ok(p.as_str()),
        SingleOrMany::Many(items) if items.len() == 1 => Ok(items[0].as_str()),
        SingleOrMany::Many(_) => Err(eyre::eyre!(
            "pack source '{id}' declares multiple paths; a pack source takes exactly one path"
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
                "bundled pack source '{id}' resolves to '{}', outside its parent '{}'",
                root.display(),
                base.display()
            ));
        }
    }
    Ok(root)
}

async fn discover_pack_manifest<TSys>(
    sys: &TSys,
    root: &Path,
) -> eyre::Result<Option<PackManifest>>
where
    TSys: FsReadAsync + Send + Sync,
{
    static NAMES: std::sync::LazyLock<Vec<String>> =
        std::sync::LazyLock::new(|| {
            omni_constants::config_file_names(omni_constants::PACK_OMNI)
        });

    for name in NAMES.iter() {
        let candidate = root.join(name);
        if sys.fs_read_async(&candidate).await.is_ok() {
            let manifest: PackManifest =
                omni_file_data_serde::read_async(candidate.as_path(), sys)
                    .await?;
            return Ok(Some(manifest));
        }
    }

    Ok(None)
}

/// Materializes the workspace's `packs:` graph into the shared store: every pack
/// root, transitively, plus each git subsystem source a pack manifest declares.
/// Built from the already-loaded pack source list and the workspace root.
pub struct PackRemoteContributor {
    sources: Vec<PackSourceConfiguration>,
    workspace_root: PathBuf,
}

impl PackRemoteContributor {
    pub fn new(
        sources: Vec<PackSourceConfiguration>,
        workspace_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            sources,
            workspace_root: workspace_root.into(),
        }
    }
}

#[async_trait]
impl<TSys> RemoteSourceContributor<TSys> for PackRemoteContributor
where
    TSys: RemoteSourceSys + FsReadAsync + Send + Sync,
{
    fn id(&self) -> &'static str {
        "pack"
    }

    async fn contribute(
        &self,
        manager: &RemoteSourceManager<TSys>,
        options: &InstallOptions,
    ) -> eyre::Result<Vec<RemoteSourceRef>> {
        let (expanded, mut refs) = expand_packs(
            manager,
            &self.sources,
            &self.workspace_root,
            options.update,
        )
        .await?;

        for node in &expanded.nodes {
            refs.extend(
                crate::materialize_flat_git(manager, &node.generators, options)
                    .await?,
            );
            refs.extend(
                crate::materialize_flat_git(manager, &node.tools, options)
                    .await?,
            );
            refs.extend(
                crate::materialize_flat_git(
                    manager,
                    &node.projections,
                    options,
                )
                .await?,
            );
        }

        Ok(refs)
    }
}
