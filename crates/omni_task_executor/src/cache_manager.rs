use derive_new::new;
use maps::UnorderedMap;
use omni_cache::{
    CachedTaskExecution, TaskExecutionInfo, impls::LocalTaskExecutionCacheStore,
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::TaskExecutionResult;

#[derive(Debug, new)]
pub struct CacheManager<'a> {
    store: &'a LocalTaskExecutionCacheStore,
    dry_run: bool,
    force: bool,
    no_cache: bool,
    replay_logs: bool,
}

impl<'a> CacheManager<'a> {
    async fn get_cached_results(
        &self,
        inputs: &[TaskExecutionInfo<'_>],
    ) -> Result<UnorderedMap<String, CachedTaskExecution>, CacheManagerError>
    {
        todo!("")
    }

    async fn cache_results(
        &self,
        results: &[TaskExecutionResult],
    ) -> Result<UnorderedMap<String, CachedTaskExecution>, CacheManagerError>
    {
        todo!("")
    }

    async fn replay_cached_logs(
        &self,
        result: &CachedTaskExecution,
    ) -> Result<(), CacheManagerError> {
        todo!("")
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{inner}")]
pub struct CacheManagerError {
    #[source]
    inner: CacheManagerErrorInner,
    kind: CacheManagerErrorKind,
}

impl CacheManagerError {
    pub fn kind(&self) -> CacheManagerErrorKind {
        self.kind
    }
}

impl<T: Into<CacheManagerErrorInner>> From<T> for CacheManagerError {
    fn from(value: T) -> Self {
        let inner = value.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(CacheManagerErrorKind), vis(pub))]
enum CacheManagerErrorInner {}
