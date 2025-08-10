use std::fmt::Display;

use crate::{CacheInfo, CachedOutput, ProjectInfo};

#[async_trait::async_trait]
pub trait TaskOutputCacheStore: Send + Sync {
    type Error: Display;

    async fn cache(&self, cache_info: &CacheInfo) -> Result<(), Self::Error> {
        self.cache_many(&[*cache_info]).await
    }

    async fn cache_many(
        &self,
        cache_infos: &[CacheInfo],
    ) -> Result<(), Self::Error>;

    async fn get(
        &self,
        project: &ProjectInfo,
    ) -> Result<Option<CachedOutput>, Self::Error> {
        Ok(self
            .get_many(&[*project])
            .await?
            .pop()
            .expect("should be some"))
    }

    async fn get_many(
        &self,
        projects: &[ProjectInfo],
    ) -> Result<Vec<Option<CachedOutput>>, Self::Error>;

    async fn invalidate_caches(
        &self,
        project_name: &str,
    ) -> Result<(), Self::Error>;
}
