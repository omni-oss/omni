use std::sync::Arc;

use async_trait::async_trait;
use derive_new::new;

use crate::{
    data::workspaces::{
        WorkspaceRepository, WorkspaceRepositoryError,
        WorkspaceRepositoryErrorInner,
    },
    data_impl::in_memory::data::InMemoryDatabase,
};

#[derive(Debug, Clone, new)]
pub struct InMemoryWorkspaceRepository {
    db: Arc<InMemoryDatabase>,
}

#[async_trait]
impl WorkspaceRepository for InMemoryWorkspaceRepository {
    async fn exists_by_code(
        &self,
        name: &str,
    ) -> Result<bool, WorkspaceRepositoryError> {
        Ok(self
            .db
            .workspaces
            .iter()
            .any(|workspace| workspace.code == name))
    }

    async fn belongs_to_organization(
        &self,
        organization_code: &str,
        workspace_code: &str,
    ) -> Result<bool, WorkspaceRepositoryError> {
        let org = self
            .db
            .organizations
            .iter()
            .find(|organization| organization.code == organization_code)
            .ok_or_else(|| {
                WorkspaceRepositoryErrorInner::OrganizationDoesNotExistByCode(
                    organization_code.to_string(),
                )
            })?;

        Ok(self.db.workspaces.iter().any(|workspace| {
            workspace.code == workspace_code
                && workspace.organization_id == org.id
        }))
    }
}
