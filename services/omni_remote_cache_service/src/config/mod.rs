use std::{hash::Hash, path::Path};

use maps::UnorderedMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sets::UnorderedSet;
use strum::{Display, EnumString, FromRepr};
use time::OffsetDateTime;

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
    pub security: SecurityConfiguration,
}

impl Configuration {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, eyre::Report> {
        let config = std::fs::read_to_string(path)?;
        let config: Configuration = serde_json::from_str(&config)?;

        Ok(config)
    }

    pub fn from_inline(config: impl AsRef<str>) -> Result<Self, eyre::Report> {
        let config: Configuration = serde_json::from_str(config.as_ref())?;

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

#[derive(
    Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema,
)]
pub struct SecurityConfiguration {
    pub api_keys: UnorderedMap<String, ApiKeyConfiguration>,
}

#[derive(
    Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq, JsonSchema,
)]
pub struct ApiKeyConfiguration {
    #[serde(default)]
    pub description: Option<String>,

    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default)]
    pub scopes: AllOrSpecificConfiguration<ScopesConfiguration>,

    #[serde(default)]
    pub organizations: AllOrSpecificConfiguration,

    #[serde(default)]
    pub workspaces: AllOrSpecificConfiguration,

    #[serde(default)]
    pub tenants: AllOrSpecificConfiguration,

    #[serde(default)]
    pub environments: AllOrSpecificConfiguration,

    #[serde(with = "time::serde::rfc3339::option")]
    #[schemars(with = "String")]
    #[serde(default)]
    pub expires_at: Option<OffsetDateTime>,
}

fn default_enabled() -> bool {
    true
}

#[derive(
    Copy,
    Clone,
    Debug,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    JsonSchema,
    Display,
    Hash,
    FromRepr,
    EnumString,
)]
pub enum ScopesConfiguration {
    #[serde(rename = "read:artifacts")]
    #[strum(serialize = "read:artifacts")]
    ReadArtifacts,

    #[serde(rename = "write:artifacts")]
    #[strum(serialize = "write:artifacts")]
    WriteArtifacts,

    #[serde(rename = "list:artifacts")]
    #[strum(serialize = "list:artifacts")]
    ListArtifacts,

    #[serde(rename = "delete:artifacts")]
    #[strum(serialize = "delete:artifacts")]
    DeleteArtifacts,
}

#[derive(
    Clone, Debug, Deserialize, Serialize, PartialEq, Eq, JsonSchema, Display,
)]
#[serde(untagged, rename_all = "kebab-case")]
pub enum AllOrSpecificConfiguration<T: Hash + Eq = String> {
    All(All),
    Specific(UnorderedSet<T>),
}

impl<T: Hash + Eq> Default for AllOrSpecificConfiguration<T> {
    fn default() -> Self {
        Self::All(All::default())
    }
}

#[derive(
    Clone,
    Debug,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    JsonSchema,
    Display,
    Default,
)]
pub enum All {
    #[default]
    #[serde(rename = "all")]
    #[strum(serialize = "all")]
    All,
}
