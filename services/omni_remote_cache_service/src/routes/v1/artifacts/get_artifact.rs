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
use crate::state::ServiceState;

#[derive(TypedPath, Deserialize)]
#[typed_path("/{digest}")]
pub struct GetArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams)]
pub struct GetArtifactQuery {
    pub ws: String,
    pub env: String,
}

#[utoipa::path(
    get,
    path = "/{digest}",
    description = "Download an artifact",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        GetArtifactQuery
    ),
    responses(
        (status = 200, description = "Success", content(
            ("application/octet-stream"),
        )),
    )
)]
pub async fn get_artifact(
    GetArtifactPath { digest }: GetArtifactPath,
    Query(query): Query<GetArtifactQuery>,
    State(state): State<ServiceState>,
) -> Response {
    let container = container(&query.ws, &query.env);
    let x = state
        .storage_backend
        .get(Some(container.as_ref()), &digest)
        .await
        .map_err(|e| InternalServerError(e));

    match x {
        Ok(o) => match o {
            Some(_) => todo!(),
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        },
        Err(e) => e.into_response(),
    }
}
