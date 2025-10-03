mod common;
mod delete_artifact;
mod get_artifact;
mod get_artifacts;
mod put_artifact;

pub use delete_artifact::*;
pub use get_artifact::*;
pub use get_artifacts::*;
pub use put_artifact::*;

use axum::Router;
use axum_extra::routing::RouterExt;
use omni_remote_cache_storage::ListItem;
use utoipa::OpenApi;

use crate::{response::data::Data, state::ServiceState};

pub fn build_router() -> Router<ServiceState> {
    Router::new()
        .typed_get(get_artifacts)
        .typed_get(get_artifact)
        .typed_put(put_artifact)
        .typed_delete(delete_artifact)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_artifacts,
        get_artifact,
        put_artifact,
        delete_artifact,
    ),
    components(
        schemas(
            Data<Vec<ListItem>>,
        )
    )
)]
pub struct ArtifactsApiDoc;
