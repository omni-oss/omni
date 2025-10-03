mod common;
mod delete_artifact;
mod get_artifact;
mod get_artifacts;
mod post_artifact;

pub use get_artifact::*;
pub use get_artifacts::*;
#[allow(unused)]
pub use post_artifact::*;
#[allow(unused)]
pub use post_artifact::*;

use axum::Router;
use axum_extra::routing::RouterExt;
use omni_remote_cache_storage::ListItem;
use utoipa::OpenApi;

use crate::{response::data::PagedData, state::ServiceState};

pub fn build_router() -> Router<ServiceState> {
    Router::new()
        .typed_get(get_artifacts)
        .typed_get(get_artifact)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_artifacts,
        get_artifact,
    ),
    components(
        schemas(
            PagedData<ListItem>,
        )
    )
)]
pub struct ArtifactsApiDoc;
