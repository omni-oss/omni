mod get_artifacts;

pub use get_artifacts::*;

use axum::Router;
use axum_extra::routing::RouterExt;
use omni_remote_cache_storage::ListItem;
use utoipa::OpenApi;

use crate::{response::data::PagedData, state::ServiceState};

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
            PagedData<ListItem>,
        )
    )
)]
pub struct ArtifactsApiDoc;
