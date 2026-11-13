//! Concrete [`RemoteSourceContributor`] implementations for each omni
//! subsystem. Each contributor is a thin, dependency-injected unit built from
//! its already-loaded configuration; it materializes its remote sources through
//! the shared [`RemoteSourceManager`] and returns the references it resolved.
//! Discovery of the configuration itself stays with the caller.

mod generator;
mod locality;
mod pack;
mod projection;
mod tool;

pub use generator::GeneratorRemoteContributor;
pub use locality::Locality;
pub use pack::{
    ExpandedPackNode, ExpandedPacks, PackContributedSource,
    PackRemoteContributor, expand_packs,
};
pub use projection::ProjectionRemoteContributor;
pub use tool::ToolRemoteContributor;

use omni_configurations::{GitSource, SourceConfig, SourceConfigProfile};
use omni_remote_source::{
    InstallOptions, RemoteSource, RemoteSourceRef,
    manager::RemoteSourceManager, sys::RemoteSourceSys,
};

/// The remote source a configured source names, or `None` for a local one.
/// The only place a [`SourceConfig`] becomes a [`RemoteSource`].
pub fn remote_source_for<P: SourceConfigProfile>(
    source: &SourceConfig<P>,
) -> Option<RemoteSource> {
    match source {
        SourceConfig::Local(_) => None,
        SourceConfig::Git(git) => Some(remote_source_for_git(git)),
        SourceConfig::Registry(_) => {
            unreachable!("registry sources are never constructed")
        }
    }
}

/// The remote source of a configured git source.
pub fn remote_source_for_git<P: SourceConfigProfile>(
    git: &GitSource<P>,
) -> RemoteSource {
    RemoteSource::Git {
        uri: git.uri.clone(),
        rev: git.rev.clone(),
    }
}

/// Materialize every remote source in a flat source list, invalidating locked
/// commits first when the caller asked to advance mutable refs. Shared by the
/// generator and tool contributors, whose source lists carry no extra fields.
async fn materialize_flat_remote<TSys, P>(
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
        if let Some(remote) = remote_source_for(source) {
            if options.update {
                manager.invalidate(&remote).await?;
            }

            let materialized = manager.materialize(&remote).await?;
            refs.push(RemoteSourceRef {
                source: remote,
                pin: materialized.pin,
            });
        }
    }

    Ok(refs)
}
