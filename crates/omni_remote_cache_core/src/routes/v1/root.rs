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

#[cfg(test)]
mod tests {
    use utoipa::OpenApi;

    use super::V1RootApiDoc;

    #[test]
    fn artifact_listing_exposes_storage_semantics_in_openapi() {
        let spec = serde_json::to_value(V1RootApiDoc::openapi()).unwrap();
        let components = &spec["components"];
        assert!(components["securitySchemes"]["bearer"].is_object());

        let item = &components["schemas"]["CacheItem"]["properties"];
        let description = |entry: &serde_json::Value| {
            entry["description"]
                .as_str()
                .expect("OpenAPI description is missing")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(description(&item["size"]).contains("trailer and footer"));
        assert!(description(&item["scope"]).contains("original scope label"));
        assert!(
            description(&spec["paths"]["/artifacts"]["get"])
                .contains("404 Not Found")
        );
    }
}
