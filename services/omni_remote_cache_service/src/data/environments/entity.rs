use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::data::workspaces::WorkspaceId;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash, new)]
pub struct Environment {
    pub id: EnvironmentId,
    pub workspace_id: WorkspaceId,

    pub code: String,
    pub display_name: String,
    pub description: Option<String>,
}

#[derive(
    Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, new,
)]
#[repr(transparent)]
pub struct EnvironmentId(pub u64);
