use axum::{
    Json,
    extract::{Query, State},
};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use omni_remote_cache_storage::{ListItem, RemoteCacheStorageBackendExt};
use serde::Deserialize;

use crate::{response::data::PagedData, state::ServiceState};

#[derive(TypedPath, Deserialize)]
#[typed_path("/")]
pub struct ArtifactsPath {}

#[derive(Deserialize)]
pub struct ArtifactsQuery {
    #[serde(default)]
    after_key: Option<u32>,
    #[serde(default)]
    per_page: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/",
    responses(
        (status = 200, description = "Success", body = PagedData<Vec<ListItem>>),
    )
)]
pub async fn get_artifacts(
    _: ArtifactsPath,
    Query(query): Query<ArtifactsQuery>,
    State(state): State<ServiceState>,
) -> Result<
    Json<PagedData<ListItem>>,
    InternalServerError<omni_remote_cache_storage::error::Error>,
> {
    let page = query.after_key.unwrap_or(1);
    let per_page = query.per_page.unwrap_or(50);
    let all_artifacts = state
        .storage_backend
        .list_default()
        .await
        .map_err(InternalServerError)?;
    let artifacts = all_artifacts
        .iter()
        .skip(((page - 1) * per_page) as usize)
        .take(per_page as usize)
        .map(|item| item.clone())
        .collect::<Vec<_>>();

    let has_next = artifacts.len() == per_page as usize;
    let has_previous = page > 1;
    let total_size = all_artifacts.len() as u32;
    let page_size = artifacts.len() as u32;

    Ok(Json(PagedData::new(
        artifacts,
        page_size,
        total_size,
        has_next,
        has_previous,
    )))
}
