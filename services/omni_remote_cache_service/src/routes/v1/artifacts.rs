use axum::{Json, Router, extract::State};
use axum_extra::{
    response::InternalServerError,
    routing::{RouterExt, TypedPath},
};
use omni_remote_cache_storage::{ListItem, RemoteCacheStorageBackendExt};
use serde::Deserialize;
use utoipa::OpenApi;

use crate::{response::data::Data, state::ServiceState};

pub fn build_router() -> Router<ServiceState> {
    Router::new().typed_get(get_artifacts)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_artifacts,
    ),
    components(
        schemas(
            Data<Vec<ListItem>>,
        )
    )
)]
pub struct ArtififactsApiDoc;

#[derive(TypedPath, Deserialize)]
#[typed_path("/")]
pub struct ArtifactsPath {}

#[utoipa::path(
    get,
    path = "/",
    responses(
        (status = 200, description = "Success", body = Data<Vec<ListItem>>),
    )
)]
async fn get_artifacts(
    _: ArtifactsPath,
    State(state): State<ServiceState>,
) -> Result<
    Json<Data<Vec<ListItem>>>,
    InternalServerError<omni_remote_cache_storage::error::Error>,
> {
    let artifacts = state
        .storage_backend
        .list_default()
        .await
        .map_err(InternalServerError)?;

    Ok(Json(Data::new(artifacts)))
}
