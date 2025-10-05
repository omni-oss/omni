use derive_new::new;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, new)]
pub struct Tenant {
    pub id: TenantId,

    pub code: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, new,
)]
#[repr(transparent)]
pub struct TenantId(pub u64);
