use axum::{
    extract::{Query, State},
    response::{IntoResponse as _, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{routes::v1::artifacts::common::container, state::ServiceState};

#[derive(TypedPath, Deserialize)]
#[typed_path("/{digest}")]
pub struct PutArtifactPath {
    pub digest: String,
}

#[derive(Deserialize, IntoParams)]
pub struct PutArtifactQuery {
    pub ws: String,
    pub env: String,
}

#[utoipa::path(
    put,
    description = "Upload an artifact",
    path = "/{digest}",
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        PutArtifactQuery
    ),
    responses(
        (status = 200, description = "Success"),
    )
)]
pub async fn put_artifact(
    PutArtifactPath { digest }: PutArtifactPath,
    Query(query): Query<PutArtifactQuery>,
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
