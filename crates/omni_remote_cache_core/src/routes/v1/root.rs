use axum::Router;
use utoipa::OpenApi;

use crate::state::ServiceState;

use super::artifacts::{self, ArtifactsApiDoc};

pub fn build_router() -> Router<ServiceState> {
    Router::new().nest("/artifacts", artifacts::build_router())
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "omni-remote-cache-service",
        // Placeholder: `open_api::apply` overwrites it with `ServiceInfo::version`.
        version = "0.0.0",
        description = "A service for caching remote artifacts",
    ),
    nest(
        (path = "/artifacts", api = ArtifactsApiDoc),
    ),
)]
pub struct V1RootApiDoc;
