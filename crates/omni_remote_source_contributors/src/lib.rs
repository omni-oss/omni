//! Concrete [`RemoteSourceContributor`] implementations for each omni
//! subsystem. Each contributor is a thin, dependency-injected unit built from
//! its already-loaded configuration; it materializes its remote sources through
//! the shared [`RemoteSourceManager`] and returns the references it resolved.
//! Discovery of the configuration itself stays with the caller.

mod generator;
mod pack;
mod projection;
mod tool;

pub use generator::GeneratorRemoteContributor;
pub use pack::{
    ExpandedPackNode, ExpandedPacks, PackContributedSource,
    PackRemoteContributor, expand_packs,
};
pub use projection::ProjectionRemoteContributor;
pub use tool::ToolRemoteContributor;

use omni_configurations::{SourceConfig, SourceConfigProfile};
use omni_remote_source::{
    InstallOptions, RemoteSource, RemoteSourceRef,
    manager::RemoteSourceManager, sys::RemoteSourceSys,
};

/// Materialize every git source in a flat source list, invalidating locked
/// commits first when the caller asked to advance mutable refs. Shared by the
/// generator and tool contributors, whose source lists carry no extra fields.
async fn materialize_flat_git<TSys, P>(
    manager: &RemoteSourceManager<TSys>,
    sources: &[SourceConfig<P>],
    options: &InstallOptions,
) -> eyre::Result<Vec<RemoteSourceRef>>
where
    TSys: RemoteSourceSys,
    P: SourceConfigProfile,
{
    let mut refs = Vec::new();

    for source in sources {
        if let SourceConfig::Git(git) = source {
            if options.update {
                manager.invalidate_git(&git.uri, &git.rev).await?;
            }

            let remote = RemoteSource::Git {
                uri: git.uri.clone(),
                rev: git.rev.clone(),
            };
            let materialized = manager.materialize(&remote).await?;
            refs.push(RemoteSourceRef {
                source: remote,
                pin: materialized.pin,
            });
        }
    }

    Ok(refs)
}
