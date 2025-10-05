use std::path::Path;

use maps::UnorderedMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    data::{
        environments::{Environment, EnvironmentId},
        organizations::{Organization, OrganizationId},
        tenants::{Tenant, TenantId},
        workspaces::{Workspace, WorkspaceId},
    },
    data_impl::in_memory::InMemoryDatabase,
};

#[derive(
    Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema,
)]
pub struct Configuration {
    pub tenants: UnorderedMap<String, TenantConfiguration>,
}

impl Configuration {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, eyre::Report> {
        let config = std::fs::read_to_string(path)?;
        let config: Configuration = serde_json::from_str(&config)?;

        Ok(config)
    }
}

impl Configuration {
    pub fn to_in_memory_database(&self) -> InMemoryDatabase {
        let mut db = InMemoryDatabase::default();

        for (tenant_id, (tenant_code, tenant_config)) in
            self.tenants.iter().enumerate()
        {
            let tenant_id = TenantId::new(tenant_id as u64);

            db.tenants.push(Tenant::new(
                tenant_id,
                tenant_code.clone(),
                tenant_config
                    .display_name
                    .clone()
                    .unwrap_or(tenant_code.to_string()),
                tenant_config.description.clone(),
            ));

            for (org_id, (org_code, org_config)) in
                tenant_config.organizations.iter().enumerate()
            {
                let org_id = OrganizationId::new(org_id as u64);

                db.organizations.push(Organization::new(
                    org_id,
                    tenant_id,
                    org_code.clone(),
                    org_config
                        .display_name
                        .clone()
                        .unwrap_or(org_code.to_string()),
                    org_config.description.clone(),
                ));

                for (ws_id, (ws_code, ws_config)) in
                    org_config.workspaces.iter().enumerate()
                {
                    let ws_id = WorkspaceId::new(ws_id as u64);

                    db.workspaces.push(Workspace::new(
                        ws_id,
                        org_id,
                        ws_code.clone(),
                        ws_config
                            .display_name
                            .clone()
                            .unwrap_or(ws_code.to_string()),
                        ws_config.description.clone(),
                    ));

                    for (env_id, (env_code, env_config)) in
                        ws_config.environments.iter().enumerate()
                    {
                        let env_id = EnvironmentId::new(env_id as u64);

                        db.environments.push(Environment::new(
                            env_id,
                            ws_id,
                            env_code.clone(),
                            env_config
                                .display_name
                                .clone()
                                .unwrap_or(env_code.to_string()),
                            env_config.description.clone(),
                        ));
                    }
                }
            }
        }

        db
    }
}

#[derive(
    Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema,
)]
pub struct TenantConfiguration {
    pub display_name: Option<String>,
    pub description: Option<String>,

    pub organizations: UnorderedMap<String, OrganizationConfiguration>,
}

#[derive(
    Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema,
)]
pub struct OrganizationConfiguration {
    pub display_name: Option<String>,
    pub description: Option<String>,

    pub workspaces: UnorderedMap<String, WorkspaceConfiguration>,
}

#[derive(
    Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema,
)]
pub struct WorkspaceConfiguration {
    pub environments: UnorderedMap<String, EnvironmentConfiguration>,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

#[derive(
    Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema,
)]
pub struct EnvironmentConfiguration {
    pub display_name: Option<String>,
    pub description: Option<String>,
}
