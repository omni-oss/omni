use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::Deserialize;
use utoipa::IntoParams;

use super::common::container;
use crate::{
    extractors::{ApiKey, TenantCode},
    routes::v1::artifacts::common::{guard, validate_ownership},
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/{digest}")]
pub struct HeadArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct HeadArtifactQuery {
    /// The organization code
    pub org: String,
    /// The workspace code
    pub ws: String,
    /// The environment code
    pub env: String,
}

#[utoipa::path(
    head,
    path = "/{digest}",
    description = "Download an artifact",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
        HeadArtifactQuery
    ),
    responses(
        (
            status = NO_CONTENT,
            description = "Success",
        ),
        (status = NOT_FOUND, description = "Not found"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip(state))]
pub async fn head_artifact(
    HeadArtifactPath { digest }: HeadArtifactPath,
    Query(query): Query<HeadArtifactQuery>,
    TenantCode(tenant_code): TenantCode,
    State(state): State<ServiceState>,
    ApiKey(api_key): ApiKey,
) -> Response {
    guard!(
        state.provider,
        &api_key,
        &tenant_code,
        &query,
        &["read:artifacts"],
    );

    validate_ownership!(state.provider, &tenant_code, &query);

    let container = container(&query.org, &query.ws, &query.env);
    let exists = state
        .storage_backend
        .exists(Some(container.as_ref()), &digest)
        .await
        .map_err(InternalServerError);

    match exists {
        Ok(o) => {
            if o {
                StatusCode::NO_CONTENT.into_response()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
        Err(e) => e.into_response(),
    }
}
