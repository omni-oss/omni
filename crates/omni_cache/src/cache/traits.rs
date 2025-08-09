use std::fmt::Display;

use crate::{CachedOutput, ProjectInfo};

#[async_trait::async_trait]
pub trait TaskOutputCacheStore: Send + Sync {
    type Error: Display;

    async fn cache(
        &self,
        project: &ProjectInfo,
        logs: Option<&str>,
    ) -> Result<(), Self::Error>;

    async fn get(
        &self,
        project: &ProjectInfo,
    ) -> Result<Option<CachedOutput>, Self::Error>;

    async fn invalidate_caches(
        &self,
        project_name: &str,
    ) -> Result<(), Self::Error>;
}
