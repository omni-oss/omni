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

/// Longest accepted `{digest}`. Real digests are base58 of a 32 byte hash
/// (at most 44 characters); the margin keeps older, longer ones working.
pub const MAX_DIGEST_LEN: usize = 64;

/// A digest becomes a storage key, so it is parsed at the boundary: base58
/// alphabet only (no `0`, `O`, `I`, `l`) and bounded. That excludes every path
/// separator, dot and percent escape, so it cannot name another location.
pub fn is_valid_digest(digest: &str) -> bool {
    !digest.is_empty()
        && digest.len() <= MAX_DIGEST_LEN
        && digest.bytes().all(|b| {
            matches!(
                b,
                b'1'..=b'9' | b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z'
                    | b'a'..=b'k' | b'm'..=b'z'
            )
        })
}

/// Rejects a malformed `{digest}` with `400` before any path is built.
macro_rules! require_valid_digest {
    ($digest:expr) => {
        if !$crate::routes::v1::artifacts::common::is_valid_digest($digest) {
            return (
                ::http::StatusCode::BAD_REQUEST,
                "invalid digest: expected base58 of at most 64 characters",
            )
                .into_response();
        }
    };
}
pub(crate) use require_valid_digest;

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

pub macro authenticate(
        $provider:expr,
        $credential:expr,
        $tenant_code:expr,
        $query:expr,
        $scope:expr$(,)?
    ) {{
    use crate::routes::v1::artifacts::common::forbidden_response;
    use crate::security::Resource;
    use axum::response::IntoResponse as _;
    let resource = Resource {
        tenant: $tenant_code.to_string(),
        org: $query.org.clone(),
        ws: $query.ws.clone(),
        env: $query.env.clone(),
    };
    match $provider
        .authenticator()
        .authenticate($credential, &resource, $scope.into())
        .await
    {
        Ok(Some(caller)) if caller.can($scope) => caller,
        Ok(Some(_)) => {
            return forbidden_response(Some(
                "You are not authorized to process this request",
            ));
        }
        Ok(None) => return crate::extractors::unauthorized_response(),
        // Never expose or log the backend report: it may contain credentials.
        Err(_) => {
            return (
                ::http::StatusCode::SERVICE_UNAVAILABLE,
                "authentication unavailable",
            )
                .into_response();
        }
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

/// Maps a response status to the outcome the `after` hook reports.
pub fn outcome_kind(
    status: StatusCode,
    success: crate::hooks::OutcomeKind,
) -> crate::hooks::OutcomeKind {
    use crate::hooks::OutcomeKind;
    if status == StatusCode::NOT_FOUND {
        OutcomeKind::Miss
    } else if status == StatusCode::CONFLICT {
        OutcomeKind::AlreadyExists
    } else if status.is_success() {
        success
    } else {
        OutcomeKind::Failed
    }
}

/// Schedules `after` from the final response and passes the response through.
pub async fn after_response(
    state: &crate::state::ServiceState,
    ctx: crate::hooks::HookContext,
    success: crate::hooks::OutcomeKind,
    response: Response,
) -> Response {
    use crate::hooks::{HookOutcome, run_after};

    let kind = outcome_kind(response.status(), success);
    run_after(state.provider.as_ref(), ctx, HookOutcome { kind }).await;

    response
}

/// Builds the hook context and runs `before`. Returns early with the veto or
/// failure response, otherwise evaluates to the context for `after`.
pub macro before_hook(
        $state:expr,
        $op:expr,
        $caller:expr,
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
        // Legacy hook field name until the slice 3 hook-contract migration.
        api_key_id: $caller.credential().id().to_owned(),
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
    fn outcome_kind_reports_conflicts_as_already_exists() {
        use super::outcome_kind;
        use crate::hooks::OutcomeKind::*;
        use http::StatusCode as S;
        assert_eq!(outcome_kind(S::NO_CONTENT, Stored), Stored);
        assert_eq!(outcome_kind(S::CONFLICT, Stored), AlreadyExists);
        assert_eq!(outcome_kind(S::NOT_FOUND, Hit), Miss);
        assert_eq!(outcome_kind(S::BAD_REQUEST, Stored), Failed);
        assert_eq!(outcome_kind(S::INTERNAL_SERVER_ERROR, Stored), Failed);
    }

    #[test]
    fn digests_must_be_bounded_base58() {
        use super::is_valid_digest as ok;
        assert!(ok("abc"));
        assert!(ok(
            "1234567891234567891234567891234567891234567891234567891234567891"
        ));
        for bad in [
            "",
            "0",
            "O",
            "I",
            "l",
            "ab-c",
            "ab.c",
            "a/b",
            "..",
            "a%2Fb",
            "a b",
            "..%2F..%2Fshared%2FX",
            "é",
        ] {
            assert!(!ok(bad), "{bad:?} must be rejected");
        }
        assert!(!ok(&"a".repeat(65)));
        assert!(ok(&"a".repeat(64)));
    }

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
