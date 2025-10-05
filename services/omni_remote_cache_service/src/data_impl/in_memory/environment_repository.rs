use std::sync::Arc;

use async_trait::async_trait;
use derive_new::new;

use crate::{
    data::environments::{
        EnvironmentRepository, EnvironmentRepositoryError,
        EnvironmentRepositoryErrorInner,
    },
    data_impl::in_memory::data::InMemoryDatabase,
};

#[derive(Debug, Clone, new)]
pub struct InMemoryEnvironmentRepository {
    db: Arc<InMemoryDatabase>,
}

#[async_trait]
impl EnvironmentRepository for InMemoryEnvironmentRepository {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, EnvironmentRepositoryError> {
        Ok(self.db.environments.iter().any(|env| env.code == code))
    }

    async fn belongs_to_workspace(
        &self,
        workspace_code: &str,
        environment_code: &str,
    ) -> Result<bool, EnvironmentRepositoryError> {
        let workspace = self
            .db
            .workspaces
            .iter()
            .find(|workspace| workspace.code == workspace_code)
            .ok_or_else(|| {
                EnvironmentRepositoryErrorInner::WorkspaceDoesNotExistByCode(
                    workspace_code.to_string(),
                )
            })?;

        Ok(self.db.environments.iter().any(|environment| {
            environment.code == environment_code
                && environment.workspace_id == workspace.id
        }))
    }
}
