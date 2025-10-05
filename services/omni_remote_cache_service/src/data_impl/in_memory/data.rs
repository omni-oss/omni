use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::data::{
    environments::Environment, organizations::Organization, tenants::Tenant,
    workspaces::Workspace,
};

#[derive(
    Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Hash, new,
)]
pub struct InMemoryDatabase {
    pub environments: Vec<Environment>,
    pub organizations: Vec<Organization>,
    pub workspaces: Vec<Workspace>,
    pub tenants: Vec<Tenant>,
}
