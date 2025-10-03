use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use http::StatusCode;
use omni_remote_cache_storage::{ListItem, RemoteCacheStorageBackendExt};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    response::data::PagedData, routes::v1::artifacts::common::key,
    state::ServiceState,
};

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
    params(
        ("digest" = String, Path, description = "Artifact digest"),
        GetArtifactQuery
    ),
    responses(
        (status = 200, description = "Success", body = PagedData<Vec<ListItem>>),
    )
)]
pub async fn get_artifact(
    GetArtifactPath { digest }: GetArtifactPath,
    Query(query): Query<GetArtifactQuery>,
    State(state): State<ServiceState>,
) -> Response {
    let key = key(&digest, &query.ws, &query.env);
    let x = state
        .storage_backend
        .get_default(&key)
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
