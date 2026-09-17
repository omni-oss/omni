use std::sync::Arc;

use omni_context::{Context, ContextSys};
use omni_remote_source::sys::RemoteSourceSys;
use serde::{Deserialize, Serialize};
use system_traits::FsReadAsync;

/// A single pack node as reported by the read-only `omni pack` commands.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PackNodeInfo {
    /// The consumer-assigned id segment.
    pub id: String,
    /// The system-composed qualified id (`<parent>::<child>`).
    pub qualified_id: String,
    /// The author-declared canonical name (display label).
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    /// The immutable git pin, if this pack came from a git source.
    pub pin: Option<String>,
    /// The subsystem faces this node effectively contributes after the
    /// `provides` gate cascade.
    pub provides: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PackListRequest;

#[derive(Debug, Clone, Default)]
pub struct PackTreeRequest;

#[derive(Debug, Clone)]
pub struct PackInfoRequest {
    /// Prune the expansion to this qualified id and its subtree.
    pub qualified_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PackListResponse {
    pub packs: Vec<PackNodeInfo>,
}

/// List the top-level packs declared in the workspace.
pub async fn handle_pack_list<TSys>(
    ctx: &Context<TSys>,
    _req: PackListRequest,
) -> eyre::Result<PackListResponse>
where
    TSys: ContextSys + RemoteSourceSys + FsReadAsync + Clone,
{
    let nodes = expand(ctx, None).await?;
    let packs = nodes
        .into_iter()
        .filter(|n| !n.qualified_id.contains("::"))
        .collect();
    Ok(PackListResponse { packs })
}

/// Report the whole expanded pack-of-packs graph.
pub async fn handle_pack_tree<TSys>(
    ctx: &Context<TSys>,
    _req: PackTreeRequest,
) -> eyre::Result<PackListResponse>
where
    TSys: ContextSys + RemoteSourceSys + FsReadAsync + Clone,
{
    Ok(PackListResponse {
        packs: expand(ctx, None).await?,
    })
}

/// Report one pack and its subtree, selected by qualified id.
pub async fn handle_pack_info<TSys>(
    ctx: &Context<TSys>,
    req: PackInfoRequest,
) -> eyre::Result<PackListResponse>
where
    TSys: ContextSys + RemoteSourceSys + FsReadAsync + Clone,
{
    let packs = expand(ctx, Some(&req.qualified_id)).await?;
    if packs.is_empty() {
        return Err(eyre::eyre!("no pack matches '{}'", req.qualified_id));
    }
    Ok(PackListResponse { packs })
}

async fn expand<TSys>(
    ctx: &Context<TSys>,
    select: Option<&str>,
) -> eyre::Result<Vec<PackNodeInfo>>
where
    TSys: ContextSys + RemoteSourceSys + FsReadAsync + Clone,
{
    let packs = ctx.workspace_configuration().packs.clone();
    if packs.is_empty() {
        return Ok(Vec::new());
    }

    let sys = ctx.sys().clone();
    let manager = Arc::new(
        crate::operations::remote_source::open_source_store(ctx, &sys).await?,
    );

    let (expanded, _refs) = omni_remote_source_contributors::expand_packs(
        manager.as_ref(),
        &packs,
        ctx.root_dir(),
        false,
        select,
    )
    .await?;

    Ok(expanded
        .nodes
        .into_iter()
        .map(|node| {
            let provides = effective_provides_labels(&node);
            PackNodeInfo {
                id: node.id,
                qualified_id: node.qualified_id,
                name: node.name,
                version: node.version,
                description: node.description,
                pin: node.pin,
                provides,
            }
        })
        .collect())
}

fn effective_provides_labels(
    node: &omni_remote_source_contributors::ExpandedPackNode,
) -> Vec<String> {
    use omni_configurations::PackSubsystem;
    use strum::VariantArray as _;

    let declares = |subsystem: PackSubsystem| match subsystem {
        PackSubsystem::Generators => !node.generators.is_empty(),
        PackSubsystem::Tools => !node.tools.is_empty(),
        PackSubsystem::Projections => !node.projections.is_empty(),
    };

    let gated = |subsystem: PackSubsystem| {
        node.effective_provides
            .as_ref()
            .is_none_or(|set| set.contains(&subsystem))
    };

    PackSubsystem::VARIANTS
        .iter()
        .copied()
        .filter(|s| declares(*s) && gated(*s))
        .map(|s| s.to_string())
        .collect()
}
