use std::fmt::Display;

use crate::{CachedTaskExecution, NewCacheInfo, TaskExecutionInfo};

#[async_trait::async_trait]
pub trait TaskExecutionCacheStore: Send + Sync {
    type Error: Display;

    async fn cache(
        &self,
        cache_info: &NewCacheInfo,
    ) -> Result<(), Self::Error> {
        self.cache_many(&[*cache_info]).await
    }

    async fn cache_many(
        &self,
        cache_infos: &[NewCacheInfo],
    ) -> Result<(), Self::Error>;

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
