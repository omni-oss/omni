mod common;
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
        self, ComponentsBuilder,
        security::{ApiKey, ApiKeyValue, SecurityScheme},
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
        ("api_key" = ["read:artifacts", "list:artifacts", "write:artifacts", "delete:artifacts"]),
    ),
    modifiers(&SecurityAddon),
)]
pub struct ArtifactsApiDoc;

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut openapi::OpenApi) {
        openapi.components = Some(
            ComponentsBuilder::new()
                .security_scheme(
                    "api_key",
                    SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new(
                        "X-API-KEY",
                    ))),
                )
                .build(),
        )
    }
}
