use axum::{
    Json,
    response::{IntoResponse, Response},
};
use http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use utoipa::{IntoParams, ToSchema};

use omni_remote_cache_storage_core::{Container, error::Error};

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
pub fn container_common(
    tenant: &str,
    query: &CommonArtifactQuery,
) -> Result<Container, Error> {
    container(tenant, &query.org, &query.ws, &query.env)
}

#[inline(always)]
pub fn container(
    tenant: &str,
    org: &str,
    ws: &str,
    env: &str,
) -> Result<Container, Error> {
    let tenant = escape_path_component(tenant);
    let org = escape_path_component(org);
    let ws = escape_path_component(ws);
    let env = escape_path_component(env);
    Container::parse(format!("{}/{}/{}/{}", tenant, org, ws, env))
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

/// Schedules `after` from the final response and passes the response through.
pub async fn after_response(
    state: &crate::state::ServiceState,
    ctx: crate::hooks::HookContext,
    success: crate::hooks::OutcomeKind,
    response: Response,
) -> Response {
    use crate::hooks::{HookOutcome, OutcomeKind, run_after};

    let status = response.status();
    let kind = if status == StatusCode::NOT_FOUND {
        OutcomeKind::Miss
    } else if status.is_success() {
        success
    } else {
        OutcomeKind::Failed
    };
    run_after(state.provider.as_ref(), ctx, HookOutcome { kind }).await;

    response
}

/// Builds the hook context and runs `before`. Returns early with the veto or
/// failure response, otherwise evaluates to the context for `after`.
pub macro before_hook(
        $state:expr,
        $op:expr,
        $api_key:expr,
        $tenant_code:expr,
        $query:expr,
        $digests:expr$(,)?
    ) {{
    use crate::hooks::{HookContext, run_before};

    let ctx = HookContext {
        op: $op,
        tenant: $tenant_code.to_string(),
        org: $query.org.clone(),
        ws: $query.ws.clone(),
        env: $query.env.clone(),
        digests: $digests,
        api_key_id: $state.provider.api_key_id($api_key),
    };

    if let Some(response) = run_before($state.provider.as_ref(), &ctx).await {
        return response;
    }

    ctx
}}

#[cfg(test)]
mod tests {
    use super::container;

    #[test]
    fn dot_and_empty_codes_are_escaped_into_a_valid_container() {
        for bad in ["", ".", ".."] {
            let c = container("t", bad, "w", "e").unwrap();
            assert!(
                !c.as_str()
                    .split('/')
                    .any(|s| s.is_empty() || s == "." || s == "..")
            );
        }
    }

    #[test]
    fn tenants_never_share_a_container() {
        assert_ne!(
            container("t1", "o", "w", "e").unwrap(),
            container("t2", "o", "w", "e").unwrap()
        );
        assert_ne!(
            container("a/b", "c", "w", "e").unwrap(),
            container("a", "b/c", "w", "e").unwrap()
        );
    }

    #[test]
    fn container_does_not_collide_on_percent_encoded_names() {
        assert_ne!(
            container("t", "foo/bar", "ws", "env").unwrap(),
            container("t", "foo%2Fbar", "ws", "env").unwrap()
        );
        assert_ne!(
            container("t", "a", "b/c", "d").unwrap(),
            container("t", "a/b", "c", "d").unwrap()
        );
        assert_ne!(
            container("t", "CON", "ws", "env").unwrap(),
            container("t", "_CON", "ws", "env").unwrap()
        );
    }
}
