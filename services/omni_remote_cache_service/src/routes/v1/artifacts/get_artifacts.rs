use axum::{
    Json,
    extract::{Query, State},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use derive_new::new;
use omni_remote_cache_storage::RemoteCacheStorageBackend;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

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

#[derive(Deserialize, Serialize, Debug, Default, new, ToSchema)]
pub struct CacheItem {
    pub digest: String,
    pub size: u64,
}

#[utoipa::path(
    get,
    description = "List artifacts",
    path = "",
    params(
        GetArtifactsQuery
    ),
    responses(
        (status = 200, description = "Success", body = Data<Vec<CacheItem>>),
        (status = INTERNAL_SERVER_ERROR, description = "Internal server error")
    )
)]
#[tracing::instrument(skip(state))]
pub async fn get_artifacts(
    _: GetArtifactsPath,
    Query(query): Query<GetArtifactsQuery>,
    State(state): State<ServiceState>,
) -> Result<
    Json<Data<Vec<CacheItem>>>,
    InternalServerError<omni_remote_cache_storage::error::Error>,
> {
    let container = container(&query.org, &query.ws, &query.env);
    let all_artifacts = state
        .storage_backend
        .list(Some(container.as_str()))
        .await
        .map_err(InternalServerError)?
        .iter()
        .map(|item| CacheItem {
            digest: item.key().to_string(),
            size: item.size().as_u64(),
        })
        .collect::<Vec<_>>();

    Ok(Json(Data::new(all_artifacts)))
}
