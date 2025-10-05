use axum::{
    Json,
    extract::{Query, State},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use omni_remote_cache_storage::{ListItem, RemoteCacheStorageBackend};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    response::data::Data, routes::v1::artifacts::common::container,
    state::ServiceState,
};

#[derive(TypedPath, Deserialize, Debug)]
#[typed_path("/")]
pub struct GetArtifactsPath {}

#[derive(Deserialize, IntoParams, Debug)]
#[into_params(parameter_in = Query)]
pub struct GetArtifactsQuery {
    #[param()]
    pub org: String,

    #[param()]
    pub ws: String,

    #[param()]
    pub env: String,
}

#[utoipa::path(
    get,
    description = "List artifacts",
    path = "",
    params(
        GetArtifactsQuery
    ),
    responses(
        (status = 200, description = "Success", body = Data<Vec<ListItem>>),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip(state))]
pub async fn get_artifacts(
    _: GetArtifactsPath,
    Query(query): Query<GetArtifactsQuery>,
    State(state): State<ServiceState>,
) -> Result<
    Json<Data<Vec<ListItem>>>,
    InternalServerError<omni_remote_cache_storage::error::Error>,
> {
    let container = container(&query.org, &query.ws, &query.env);
    let all_artifacts = state
        .storage_backend
        .list(Some(container.as_str()))
        .await
        .map_err(InternalServerError)?;

    Ok(Json(Data::new(all_artifacts)))
}
