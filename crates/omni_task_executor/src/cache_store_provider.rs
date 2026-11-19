use derive_new::new;
use omni_cache::{
    TaskExecutionCacheStore,
    impls::{
        EnabledRemoteConfig, HybridTaskExecutionCacheStore,
        LocalTaskExecutionCacheStoreError, RemoteConfig,
    },
};
use omni_context::{ContextSys, LoadedContext};

pub trait CacheStoreProvider {
    type CacheStoreError;
    type CacheStore: TaskExecutionCacheStore<Error = Self::CacheStoreError>;

    fn get_cache_store(&self) -> Self::CacheStore;

    /// A store that never uses the remote cache, for tasks that opted out.
    fn get_local_cache_store(&self) -> Self::CacheStore;
}

#[derive(Debug, Clone, new)]
pub struct ContextCacheStoreProvider<'a, TSys: ContextSys> {
    context: &'a LoadedContext<TSys>,
}

impl<'a, TSys: ContextSys> CacheStoreProvider
    for ContextCacheStoreProvider<'a, TSys>
{
    type CacheStoreError = LocalTaskExecutionCacheStoreError;
    type CacheStore = HybridTaskExecutionCacheStore;

    fn get_cache_store(&self) -> Self::CacheStore {
        let cache_dir = self.context.cache_dir();
        let remote_config = if let Some(rc) =
            &self.context.remote_cache_configuration()
        {
            RemoteConfig::new_enabled(EnabledRemoteConfig::from_configuration(
                rc,
                &self.context.workspace_configuration().remote_cache,
            ))
        } else {
            RemoteConfig::new_disabled()
        };

        HybridTaskExecutionCacheStore::new(
            cache_dir,
            self.context.root_dir(),
            remote_config,
        )
    }

    fn get_local_cache_store(&self) -> Self::CacheStore {
        HybridTaskExecutionCacheStore::new_local_only(
            self.context.cache_dir(),
            self.context.root_dir(),
        )
    }
}
