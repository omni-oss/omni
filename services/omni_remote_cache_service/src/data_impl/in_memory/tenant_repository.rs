use std::sync::Arc;

use async_trait::async_trait;
use derive_new::new;

use crate::{
    data::tenants::{TenantRepository, TenantRepositoryError},
    data_impl::in_memory::data::InMemoryDatabase,
};

#[derive(Debug, Clone, new)]
pub struct InMemoryTenantRepository {
    db: Arc<InMemoryDatabase>,
}

#[async_trait]
impl TenantRepository for InMemoryTenantRepository {
    async fn exists_by_code(
        &self,
        code: &str,
    ) -> Result<bool, TenantRepositoryError> {
        Ok(self.db.tenants.iter().any(|tenant| tenant.code == code))
    }
}
