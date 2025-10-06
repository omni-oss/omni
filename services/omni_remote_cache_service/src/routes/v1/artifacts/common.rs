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
                "type": "https://httpstatuses.com/400",
                "title": "Validation Errors",
                "detail": violations.join(".\n"),
                "instance": "",
                "status": StatusCode::BAD_REQUEST.as_u16(),
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

pub fn forbidden_response(message: Option<&str>) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "type": "https://httpstatuses.com/403",
            "title": "Forbidden",
            "detail": message.unwrap_or("Forbidden"),
            "instance": "",
            "status": StatusCode::FORBIDDEN.as_u16(),
        })),
    )
        .into_response()
}

pub macro guard(
        $provider:expr,
        $api_key:expr,
        $tenant_code:expr,
        $query:expr,
        $scopes:expr$(,)?
    ) {{
    use crate::routes::v1::artifacts::common::forbidden_response;
    use axum::response::IntoResponse as _;
    use axum_extra::response::InternalServerError;

    let security = $provider.security_service();

    let security_result = security
        .can_access(
            $api_key,
            $tenant_code,
            &$query.org,
            &$query.ws,
            &$query.env,
            $scopes,
        )
        .await
        .map_err(InternalServerError);

    match security_result {
        Ok(true) => (),
        Ok(false) => {
            return forbidden_response(Some(
                "You are not authorized to process this request",
            ));
        }
        Err(e) => return e.into_response(),
    }
}}

pub macro validate_ownership(
        $provider:expr,
        $tenant_code:expr,
        $query:expr$(,)?
    ) {{
    use crate::routes::v1::artifacts::common::get_validation_response;
    use axum::response::IntoResponse as _;
    use axum_extra::response::InternalServerError;

    let validate_svc = $provider.validation_service();

    let result = validate_svc
        .validate_ownership($tenant_code, &$query.org, &$query.ws, &$query.env)
        .await
        .map_err(InternalServerError);

    match result {
        Ok(r) => {
            if let Some(response) = get_validation_response(
                r.violations(),
                &$tenant_code,
                &$query.org,
                &$query.ws,
                &$query.env,
            ) {
                return response;
            }
        }
        Err(e) => return e.into_response(),
    }
}}
