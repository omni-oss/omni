use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Operator-supplied SQL, in the driver's own dialect. Each query returns
/// zero or one row with a `found` column (boolean or integer). Parameters
/// are bound in the documented order and nothing is rewritten.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct QueriesConfiguration {
    /// params: tenant
    pub tenant_exists: String,
    /// params: organization
    pub organization_exists: String,
    /// params: tenant, organization
    pub organization_belongs_to_tenant: String,
    /// params: workspace
    pub workspace_exists: String,
    /// params: organization, workspace
    pub workspace_belongs_to_organization: String,
    /// params: environment
    pub environment_exists: String,
    /// params: workspace, environment
    pub environment_belongs_to_workspace: String,
    /// params: api_key (after hashing)
    pub api_key_valid: String,
    /// params: api_key, tenant
    pub api_key_tenant: String,
    /// params: api_key, tenant, organization
    pub api_key_organization: String,
    /// params: api_key, tenant, organization, workspace
    pub api_key_workspace: String,
    /// params: api_key, tenant, organization, workspace, environment
    pub api_key_environment: String,
    /// params: api_key, scope (for example `read:artifacts`)
    pub api_key_scope: String,
}

/// A query with the parameter count and result column it must have.
pub struct QuerySpec<'a> {
    pub name: &'static str,
    pub sql: &'a str,
    pub params: usize,
    pub column: Option<&'static str>,
}

impl QueriesConfiguration {
    pub fn specs(&self) -> Vec<QuerySpec<'_>> {
        let q = |name, sql, params| QuerySpec {
            name,
            sql,
            params,
            column: Some("found"),
        };
        vec![
            q("tenant_exists", &self.tenant_exists, 1),
            q("organization_exists", &self.organization_exists, 1),
            q(
                "organization_belongs_to_tenant",
                &self.organization_belongs_to_tenant,
                2,
            ),
            q("workspace_exists", &self.workspace_exists, 1),
            q(
                "workspace_belongs_to_organization",
                &self.workspace_belongs_to_organization,
                2,
            ),
            q("environment_exists", &self.environment_exists, 1),
            q(
                "environment_belongs_to_workspace",
                &self.environment_belongs_to_workspace,
                2,
            ),
            q("api_key_valid", &self.api_key_valid, 1),
            q("api_key_tenant", &self.api_key_tenant, 2),
            q("api_key_organization", &self.api_key_organization, 3),
            q("api_key_workspace", &self.api_key_workspace, 4),
            q("api_key_environment", &self.api_key_environment, 5),
            q("api_key_scope", &self.api_key_scope, 2),
        ]
    }

    pub fn validate(&self) -> Result<(), String> {
        let all = [
            ("tenant_exists", &self.tenant_exists),
            ("organization_exists", &self.organization_exists),
            (
                "organization_belongs_to_tenant",
                &self.organization_belongs_to_tenant,
            ),
            ("workspace_exists", &self.workspace_exists),
            (
                "workspace_belongs_to_organization",
                &self.workspace_belongs_to_organization,
            ),
            ("environment_exists", &self.environment_exists),
            (
                "environment_belongs_to_workspace",
                &self.environment_belongs_to_workspace,
            ),
            ("api_key_valid", &self.api_key_valid),
            ("api_key_tenant", &self.api_key_tenant),
            ("api_key_organization", &self.api_key_organization),
            ("api_key_workspace", &self.api_key_workspace),
            ("api_key_environment", &self.api_key_environment),
            ("api_key_scope", &self.api_key_scope),
        ];
        for (name, sql) in all {
            if sql.trim().is_empty() {
                return Err(format!("query `{name}` is empty"));
            }
        }
        Ok(())
    }
}
