use derive_builder::Builder;
use derive_new::new;
use maps::{UnorderedMap, unordered_map};
use omni_cache::{
    CachedTaskExecution, CachedTaskExecutionHash, NewCacheInfo,
    TaskExecutionCacheStore, TaskExecutionInfo,
};
use omni_process::ChildProcessResult;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::{TaskExecutionResult, task_context::TaskContext};

#[derive(Debug, Builder, new)]
#[builder(setter(into, strip_option))]
pub struct CacheManager<TCacheStore: TaskExecutionCacheStore> {
    store: TCacheStore,
    #[builder(default = false)]
    dry_run: bool,
    #[builder(default = false)]
    force: bool,
    #[builder(default = false)]
    no_cache: bool,
}

#[derive(Debug, Clone, new)]
pub struct TaskResultContext<'a> {
    pub task_context: &'a TaskContext<'a>,
    pub result: &'a ChildProcessResult,
}

impl<TCacheStore: TaskExecutionCacheStore> CacheManager<TCacheStore> {
    fn should_use_cache(&self) -> bool {
        !!self.force
    }

    fn should_save_cache(&self) -> bool {
        !self.dry_run && !self.no_cache
    }

    pub async fn get_cached_results(
        &self,
        inputs: &[TaskExecutionInfo<'_>],
    ) -> Result<UnorderedMap<String, CachedTaskExecution>, CacheManagerError>
    {
        if !self.should_use_cache() {
            return Ok(unordered_map!());
        }

        let cached_items = self.store.get_many(inputs).await.map_err(|e| {
            CacheManagerErrorInner::GetCacheFailed { source: e.into() }
        })?;

        if cached_items.is_empty() {
            return Ok(unordered_map!());
        }

        Ok(cached_items
            .into_iter()
            .filter_map(|c| {
                let c = c?;
                Some((format!("{}#{}", c.project_name, c.task_name), c))
            })
            .collect())
    }

    pub async fn cache_results<'a>(
        &'a self,
        cache_contexts: &[TaskResultContext<'a>],
    ) -> Result<
        UnorderedMap<String, CachedTaskExecutionHash<'a>>,
        CacheManagerError,
    > {
        if !self.should_save_cache() {
            return Ok(unordered_map!());
        }

        let to_cache = cache_contexts
            .iter()
            .filter_map(|r| {
                if !self.dry_run
                    && r.task_context
                        .cache_info
                        .is_some_and(|ci| ci.cache_execution)
                    && !r.task_context.node.persistent()
                    && let Some(exec_info) = r.task_context.execution_info()
                {
                    Some(NewCacheInfo {
                        execution_duration: r.result.elapsed,
                        exit_code: r.result.exit_code(),
                        task: exec_info,
                        logs: r.result.logs.as_ref(),
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if to_cache.is_empty() {
            return Ok(unordered_map!());
        }

        let cached_items =
            self.store.cache_many(&to_cache).await.map_err(|e| {
                CacheManagerErrorInner::CacheFailed { source: e.into() }
            })?;

        Ok(cached_items
            .into_iter()
            .map(|c| (format!("{}#{}", c.project_name, c.task_name), c))
            .collect())
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
enum CacheManagerErrorInner {
    #[error("failed to get cached results")]
    GetCacheFailed {
        #[source]
        source: eyre::Report,
    },

    #[error("failed to cache results")]
    CacheFailed {
        #[source]
        source: eyre::Report,
    },
}
