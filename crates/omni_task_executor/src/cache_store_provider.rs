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
        let remote_config =
            if let Some(rc) = &self.context.remote_cache_configuration() {
                RemoteConfig::new_enabled(EnabledRemoteConfig::new(
                    rc.api_base_url.as_str(),
                    rc.api_key.as_str(),
                    rc.tenant_code.as_str(),
                    rc.organization_code.as_str(),
                    rc.workspace_code.as_str(),
                    rc.environment_code.clone(),
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
}
