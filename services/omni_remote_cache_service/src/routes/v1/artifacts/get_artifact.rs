use std::convert::Infallible;

use axum::{
    body::Body,
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::{StatusCode, header};
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::Deserialize;
use tokio_stream::StreamExt;
use utoipa::IntoParams;

use super::common::container;
use crate::{
    extractors::TenantCode,
    routes::v1::artifacts::common::get_validation_response,
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/{digest}")]
pub struct GetArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct GetArtifactQuery {
    pub org: String,
    pub ws: String,
    pub env: String,
}

#[utoipa::path(
    get,
    path = "/{digest}",
    description = "Download an artifact",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        ("x-omni-tenant" = String, Header, description = "Tenant code"),
        GetArtifactQuery
    ),
    responses(
        (
            status = OK,
            description = "Success",
            content_type = "application/octet-stream",
        ),
        (status = NOT_FOUND, description = "Not found"),
        (status = BAD_REQUEST, description = "Bad request"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip(state))]
pub async fn get_artifact(
    GetArtifactPath { digest }: GetArtifactPath,
    Query(query): Query<GetArtifactQuery>,
    TenantCode(tenant_code): TenantCode,
    State(state): State<ServiceState>,
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
        .get_stream(Some(container.as_ref()), &digest)
        .await
        .map_err(|e| InternalServerError(e));

    match x {
        Ok(o) => match o {
            Some(stream) => {
                let body =
                    Body::from_stream(stream.map(|e| Ok::<_, Infallible>(e)));

                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/octet-stream")
                    .header(
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", digest),
                    )
                    .body(body)
                    .expect("should be able to build response from stream")
            }
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        },
        Err(e) => e.into_response(),
    }
}
