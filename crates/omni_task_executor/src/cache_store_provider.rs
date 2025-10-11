use derive_new::new;
use omni_cache::{
    TaskExecutionCacheStore,
    impls::{
        HybridTaskExecutionCacheStore, LocalTaskExecutionCacheStoreError,
        RemoteConfig,
    },
};
use omni_context::{ContextSys, LoadedContext};

pub trait CacheStoreProvider {
    type CacheStoreError;
    type CacheStore: TaskExecutionCacheStore<Error = Self::CacheStoreError>;

    fn get_cache_store(&self) -> Self::CacheStore;
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
        let cache_dir = self.context.root_dir().join(".omni/cache");
        HybridTaskExecutionCacheStore::new(
            cache_dir,
            self.context.root_dir(),
            RemoteConfig::new_disabled(),
        )
    }
}
