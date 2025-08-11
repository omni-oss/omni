use std::fmt::Display;

use crate::{
    CachedTaskExecution, CachedTaskExecutionHash, NewCacheInfo,
    TaskExecutionInfo,
};

#[async_trait::async_trait]
pub trait TaskExecutionCacheStore: Send + Sync {
    type Error: Display;

    async fn cache<'a>(
        &'a self,
        cache_info: &'a NewCacheInfo<'a>,
    ) -> Result<CachedTaskExecutionHash<'a>, Self::Error> {
        let results = self.cache_many(&[cache_info]).await?;
        let first = results[0];
        Ok(first)
    }

    async fn cache_many<'a>(
        &'a self,
        cache_infos: &[&'a NewCacheInfo<'a>],
    ) -> Result<Vec<CachedTaskExecutionHash<'a>>, Self::Error>;

    async fn get(
        &self,
        task_infos: &TaskExecutionInfo,
    ) -> Result<Option<CachedTaskExecution>, Self::Error> {
        Ok(self
            .get_many(&[*task_infos])
            .await?
            .pop()
            .expect("should be some"))
    }

    async fn get_many(
        &self,
        task_infos: &[TaskExecutionInfo],
    ) -> Result<Vec<Option<CachedTaskExecution>>, Self::Error>;

    async fn invalidate_caches(
        &self,
        project_name: &str,
    ) -> Result<(), Self::Error>;
}
