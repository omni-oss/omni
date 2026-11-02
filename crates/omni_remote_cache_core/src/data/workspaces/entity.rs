use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::data::organizations::OrganizationId;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, new)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub organization_id: OrganizationId,

    pub code: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, new,
)]
#[repr(transparent)]
pub struct WorkspaceId(pub u64);
