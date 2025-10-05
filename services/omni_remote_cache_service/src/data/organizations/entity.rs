use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::data::tenants::TenantId;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, new)]
pub struct Organization {
    pub id: OrganizationId,
    pub tenant_id: TenantId,

    pub code: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, new,
)]
#[repr(transparent)]
pub struct OrganizationId(pub u64);
