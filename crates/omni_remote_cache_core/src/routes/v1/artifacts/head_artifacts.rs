use axum::{
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::routing::TypedPath;
use http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::{Credential, TenantCode},
    hooks::{ArtifactOp, OutcomeKind},
    routes::v1::artifacts::common::{
        after_response, authenticate, before_hook, validate_ownership,
    },
    security::Scope,
    state::ServiceState,
};

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
        (status = NO_CONTENT, description = "Success"),
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
    let caller = authenticate!(
        state.provider,
        &credential,
        &tenant_code,
        &query,
        Scope::List,
    );

    validate_ownership!(state.provider, &tenant_code, &query);

    let hook_ctx = before_hook!(
        state,
        ArtifactOp::HeadMany,
        &caller,
        &tenant_code,
        &query,
        Vec::new(),
    );

    let response = StatusCode::NO_CONTENT.into_response();

    after_response(&state, hook_ctx, OutcomeKind::Hit, response).await
}
