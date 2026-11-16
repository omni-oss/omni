use axum::{
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::routing::TypedPath;
use http::{HeaderName, HeaderValue, StatusCode};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        CommonArtifactQuery, after_response, authenticate_caller, before_hook,
        not_authorized_response, validate_ownership,
    },
    security::{Scope, Scopes},
    state::ServiceState,
};

/// Effective scopes the credential holds for the target (`read:artifacts,...`).
pub const HEADER_SCOPES: HeaderName = HeaderName::from_static("x-omni-scopes");

/// The largest artifact this server accepts, in bytes.
pub const HEADER_MAX_ARTIFACT_SIZE: HeaderName =
    HeaderName::from_static("x-omni-max-artifact-size");

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/")]
pub struct HeadArtifactsPath {}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct HeadArtifactsQuery {
    #[param()]
    /// The organization code
    pub org: String,

    #[param()]
    /// The workspace code
    pub ws: String,

    #[param()]
    /// The environment code
    pub env: String,
}

#[utoipa::path(
    head,
    description = "List artifacts",
    path = "",
    params(
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
        HeadArtifactsQuery
    ),
    responses(
        (status = NO_CONTENT, description = "Success. `X-Omni-Scopes` lists the scopes this credential holds for the target and `X-Omni-Max-Artifact-Size` the largest accepted artifact in bytes"),
        (status = UNAUTHORIZED, description = "Missing or unrecognized credential"),
        (status = FORBIDDEN, description = "The credential holds no scope for the target"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip_all, fields(org = %query.org, ws = %query.ws, env = %query.env))]
pub async fn head_artifacts(
    _: HeadArtifactsPath,
    Query(query): Query<HeadArtifactsQuery>,
    State(state): State<ServiceState>,
    TenantCode(tenant_code): TenantCode,
    Credential(credential): Credential,
) -> Response {
    // This is the access probe `omni cache remote setup` uses: it asks for every
    // scope and reports the ones this credential holds for the target, so any
    // recognized caller with at least one scope is valid.
    let caller = match authenticate_caller(
        state.provider.as_ref(),
        &credential,
        &tenant_code,
        &CommonArtifactQuery {
            org: query.org.clone(),
            ws: query.ws.clone(),
            env: query.env.clone(),
        },
        Scopes::ALL,
    )
    .await
    {
        Ok(caller) if !caller.scopes().is_empty() => caller,
        Ok(_) => return not_authorized_response(),
        Err(response) => return *response,
    };

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::HeadMany,
        &caller,
        &tenant_code,
        &query,
        Vec::new(),
    );

    let mut response = StatusCode::NO_CONTENT.into_response();
    let scopes = caller
        .scopes()
        .iter()
        .map(Scope::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&scopes) {
        headers.insert(HEADER_SCOPES, value);
    }
    headers.insert(
        HEADER_MAX_ARTIFACT_SIZE,
        HeaderValue::from(state.max_artifact_size),
    );

    after_response(&state, hook_ctx, OutcomeKind::Hit, response).await
}
