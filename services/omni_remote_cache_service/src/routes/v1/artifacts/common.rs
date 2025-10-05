use axum::{
    Json,
    response::{IntoResponse, Response},
};
use http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use crate::{services::Violation, utils::path::escape_path_component};

#[allow(unused)]
#[derive(Deserialize, IntoParams, Debug, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct CommonArtifactQuery {
    pub org: String,
    pub ws: String,
    pub env: String,
}

#[allow(unused)]
#[inline(always)]
pub fn container_common(query: &CommonArtifactQuery) -> String {
    container(&query.org, &query.ws, &query.env)
}

#[inline(always)]
pub fn container(org: &str, ws: &str, env: &str) -> String {
    let org = escape_path_component(org);
    let ws = escape_path_component(ws);
    let env = escape_path_component(env);
    format!("{}/{}/{}", org, ws, env)
}

#[inline(always)]
pub fn get_validation_response(
    violations: &[Violation],
    tenant: &str,
    org: &str,
    ws: &str,
    env: &str,
) -> Option<Response> {
    if violations.is_empty() {
        return None;
    }

    let violations = violations
        .iter()
        .copied()
        .map(|v| translate_violation(v, tenant, org, ws, env))
        .collect::<Vec<_>>();

    Some(
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Validation errors",
                "code": StatusCode::BAD_REQUEST.as_u16(),
                "violations": violations
            })),
        )
            .into_response(),
    )
}

fn translate_violation(
    violation: Violation,
    tenant: &str,
    org: &str,
    ws: &str,
    env: &str,
) -> String {
    match violation {
        Violation::TenantDoesNotExist => {
            format!("tenant for code '{}' does not exist", tenant)
        }
        Violation::TenantDoesNotHaveOrganization => {
            format!(
                "tenant for code '{}' does not have an organization with code '{}'",
                org, org
            )
        }
        Violation::OrganizationDoesNotExist => {
            format!("organization for code '{}' does not exist", org)
        }
        Violation::OrganizationDoesNotHaveWorkspace => {
            format!(
                "organization for code '{}' does not have a workspace with code '{}'",
                org, ws
            )
        }
        Violation::WorkspaceDoesNotExist => {
            format!("workspace for code '{}' does not exist", ws)
        }
        Violation::WorkspaceDoesNotHaveEnvironment => {
            format!(
                "workspace for code '{}' does not have an environment with code '{}'",
                ws, env
            )
        }
        Violation::EnvironmentDoesNotExist => {
            format!("environment for code '{}' does not exist", env)
        }
    }
}
