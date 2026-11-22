pub(crate) mod common;
mod delete_artifact;
mod get_artifact;
mod get_artifacts;
mod head_artifact;
mod head_artifacts;
mod put_artifact;

pub use delete_artifact::*;
pub use get_artifact::*;
pub use get_artifacts::*;
pub use head_artifact::*;
pub use head_artifacts::*;
pub use put_artifact::*;

use axum::Router;
use axum_extra::routing::RouterExt;
use utoipa::{
    Modify, OpenApi,
    openapi::{
        self,
        security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
    },
};

use crate::{response::data::Data, state::ServiceState};

pub fn build_router() -> Router<ServiceState> {
    Router::new()
        .typed_get(get_artifacts)
        .typed_head(head_artifacts)
        .typed_get(get_artifact)
        .typed_put(put_artifact)
        .typed_delete(delete_artifact)
        .typed_head(head_artifact)
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_artifacts,
        get_artifact,
        put_artifact,
        delete_artifact,
        head_artifact,
        head_artifacts,
    ),
    components(
        schemas(
            Data<Vec<CacheItem>>,
        )
    ),
    security(
        ("bearer" = []),
    ),
    modifiers(&SecurityAddon),
)]
pub struct ArtifactsApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut openapi::OpenApi) {
        openapi
            .components
            .get_or_insert_default()
            .add_security_scheme(
                "bearer",
                SecurityScheme::Http(
                    HttpBuilder::new().scheme(HttpAuthScheme::Bearer).build(),
                ),
            )
    }
}
