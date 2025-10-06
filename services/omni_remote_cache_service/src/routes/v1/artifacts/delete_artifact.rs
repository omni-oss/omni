use axum::{
    debug_handler,
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    extractors::{ApiKey, TenantCode},
    routes::v1::artifacts::common::{
        container, get_validation_response, guard,
    },
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
    /// The organization code
    pub org: String,
    /// The workspace code
    pub ws: String,
    /// The environment code
    pub env: String,
}

#[utoipa::path(
    delete,
    description = "Delete an artifact",
    path = "/{digest}",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        ("X-OMNI-TENANT" = String, Header, description = "Tenant code"),
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
#[debug_handler]
pub async fn delete_artifact(
    DeleteArtifactPath { digest }: DeleteArtifactPath,
    Query(query): Query<DeleteArtifactQuery>,
    State(state): State<ServiceState>,
    TenantCode(tenant_code): TenantCode,
    ApiKey(api_key): ApiKey,
) -> Response {
    guard!(
        state.provider,
        &api_key,
        &tenant_code,
        &query,
        &["delete:artifacts"]
    );

    let validate_svc = state.provider.validation_service();

    let result = validate_svc
        .validate_ownership(&tenant_code, &query.org, &query.ws, &query.env)
        .await
        .map_err(InternalServerError);

    match result {
        Ok(r) => {
            if let Some(response) = get_validation_response(
                r.violations(),
                &tenant_code,
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
