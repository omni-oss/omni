use axum::{
    Json,
    extract::FromRequestParts,
    response::{IntoResponse, Response},
};
use http::{StatusCode, request::Parts};
use serde_json::json;

pub struct ApiKey(pub String);

pub enum ApiKeyRejection {
    NoApiKey,
    CantParseApiKey(String),
}

impl IntoResponse for ApiKeyRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            match self {
                ApiKeyRejection::NoApiKey => Json(json!({
                    "type": "https://httpstatuses.com/400",
                    "title": "No API Key Provided",
                    "detail": "No API key provided",
                    "status": StatusCode::BAD_REQUEST.as_u16(),
                    "instance": "",
                })),
                ApiKeyRejection::CantParseApiKey(e) => Json(json!({
                    "type": "https://httpstatuses.com/400",
                    "title": "Can't Parse API Key",
                    "detail": format!("Can't Parse API Key: {}", e),
                    "status": StatusCode::BAD_REQUEST.as_u16(),
                    "instance": "",
                })),
            },
        )
            .into_response()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for ApiKey {
    /// The rejection type.
    /// This is the type that is returned when the extractor fails.
    type Rejection = ApiKeyRejection;

    /// Perform the extraction.
    async fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("X-API-KEY")
            .map(|code| match code.to_str() {
                Ok(o) => Ok(ApiKey(o.to_string())),
                Err(e) => Err(ApiKeyRejection::CantParseApiKey(e.to_string())),
            })
            .ok_or(ApiKeyRejection::NoApiKey)?
    }
}
