use std::sync::Arc;

use async_trait::async_trait;
use derive_new::new;

use crate::{
    data::organizations::{
        OrganizationRepository, OrganizationRepositoryError,
        OrganizationRepositoryErrorInner,
    },
    data_impl::in_memory::data::InMemoryDatabase,
};

#[derive(Debug, Clone, new)]
pub struct InMemoryOrganizationRepository {
    db: Arc<InMemoryDatabase>,
}

#[async_trait]
impl OrganizationRepository for InMemoryOrganizationRepository {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, OrganizationRepositoryError> {
        Ok(self.db.organizations.iter().any(|org| org.code == code))
    }

    async fn belongs_to_tenant(
        &self,
        tenant_code: &str,
        organization_code: &str,
    ) -> Result<bool, OrganizationRepositoryError> {
        let tenant = self
            .db
            .tenants
            .iter()
            .find(|tenant| tenant.code == tenant_code)
            .ok_or_else(|| {
                OrganizationRepositoryErrorInner::TenantDoesNotExistByCode(
                    tenant_code.to_string(),
                )
            })?;

        Ok(self.db.organizations.iter().any(|organization| {
            organization.code == organization_code
                && organization.tenant_id == tenant.id
        }))
    }
}
