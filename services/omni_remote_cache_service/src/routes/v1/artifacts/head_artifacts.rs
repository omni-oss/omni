use axum::{
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::routing::TypedPath;
use http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::{ApiKey, TenantCode},
    routes::v1::artifacts::common::{guard, validate_ownership},
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
#[tracing::instrument(skip(state))]
pub async fn head_artifacts(
    _: HeadArtifactsPath,
    Query(query): Query<HeadArtifactsQuery>,
    State(state): State<ServiceState>,
    TenantCode(tenant_code): TenantCode,
    ApiKey(api_key): ApiKey,
) -> Response {
    guard!(
        state.provider,
        &api_key,
        &tenant_code,
        &query,
        &["list:artifacts"],
    );

    validate_ownership!(state.provider, &tenant_code, &query);

    StatusCode::NO_CONTENT.into_response()
}
