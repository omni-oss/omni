use crate::{
    error::Error, manager::RemoteSourceManager, source::RemoteSourceRef,
    sys::RemoteSourceSys,
};

/// Options that steer an install pass across all contributors.
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Advance mutable refs by re-resolving branches or tags and re-pinning
    /// them, with the same meaning as `projection sync --update`.
    pub update: bool,
}

/// A subsystem that contributes remote sources to the shared store. Each
/// implementor captures the already-loaded configuration it needs at
/// construction, materializes its sources through the shared manager, and
/// returns the references it resolved. The manager owns all file I/O (pin
/// lockfile, reference sets), so the trait stays object-safe and enabled
/// contributors can be held as `Vec<Box<dyn RemoteSourceContributor<TSys>>>`.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait RemoteSourceContributor<TSys: RemoteSourceSys>: Send + Sync {
    /// A stable identifier used to name this subsystem's reference set on disk.
    fn id(&self) -> &'static str;

    /// Materialize this subsystem's remote sources through the shared manager
    /// and return every reference resolved, for the manager to persist as a
    /// garbage-collection root.
    async fn contribute(
        &self,
        manager: &RemoteSourceManager<TSys>,
        options: &InstallOptions,
    ) -> Result<Vec<RemoteSourceRef>, Error>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_traits::impls::InMemorySys;

    #[test]
    fn contributor_is_object_safe() {
        fn assert_object_safe(_: &dyn RemoteSourceContributor<InMemorySys>) {}

        let mock = MockRemoteSourceContributor::<InMemorySys>::new();
        assert_object_safe(&mock);
    }
}
