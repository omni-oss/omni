use axum::{
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::TenantCode,
    routes::v1::artifacts::common::{container, get_validation_response},
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/{digest}")]
pub struct DeleteArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct DeleteArtifactQuery {
    pub org: String,
    pub ws: String,
    pub env: String,
}

#[utoipa::path(
    delete,
    description = "Delete an artifact",
    path = "/{digest}",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        DeleteArtifactQuery
    ),
    responses(
        (status = NO_CONTENT, description = "Success"),
        (status = NOT_FOUND, description = "Not found"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip(state))]
pub async fn delete_artifact(
    DeleteArtifactPath { digest }: DeleteArtifactPath,
    Query(query): Query<DeleteArtifactQuery>,
    State(state): State<ServiceState>,
    TenantCode(tenant_code): TenantCode,
) -> Response {
    let validate_svc = state.provider.validation_service();

    let result = validate_svc
        .validate_ownership(&tenant_code, &query.org, &query.ws, &query.env)
        .await
        .map_err(InternalServerError);

    match result {
        Ok(r) => {
            if let Some(response) = get_validation_response(
                r.violations(),
                &query.org,
                &query.ws,
                &query.env,
            ) {
                return response;
            }
        }
        Err(e) => return e.into_response(),
    }

    let container = container(&query.org, &query.ws, &query.env);
    let x = state
        .storage_backend
        .delete(Some(container.as_ref()), &digest)
        .await
        .map_err(|e| InternalServerError(e));

    match x {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}
