use axum::{
    Json,
    extract::FromRequestParts,
    response::{IntoResponse, Response},
};
use http::{StatusCode, request::Parts};
use serde_json::json;

pub struct TenantCode(pub String);

pub enum TenantCodeRejection {
    NoTenantCode,
    CantParseTenantCode(String),
}

impl IntoResponse for TenantCodeRejection {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            match self {
                TenantCodeRejection::NoTenantCode => Json(json!({
                    "type": "https://httpstatuses.com/400",
                    "title": "No Tenant Code Provided",
                    "detail": "No tenant code provided",
                    "status": StatusCode::BAD_REQUEST.as_u16(),
                    "instance": "",
                })),
                TenantCodeRejection::CantParseTenantCode(e) => Json(json!({
                    "type": "https://httpstatuses.com/400",
                    "title": "Can't Parse Tenant Code",
                    "detail": format!("Can't parse tenant code: {}", e),
                    "status": StatusCode::BAD_REQUEST.as_u16(),
                    "instance": "",
                })),
            },
        )
            .into_response()
    }
}

impl<S: Send + Sync> FromRequestParts<S> for TenantCode {
    /// The rejection type.
    /// This is the type that is returned when the extractor fails.
    type Rejection = TenantCodeRejection;

    /// Perform the extraction.
    async fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get("X-OMNI-TENANT")
            .map(|code| match code.to_str() {
                Ok(o) => Ok(TenantCode(o.to_string())),
                Err(e) => {
                    Err(TenantCodeRejection::CantParseTenantCode(e.to_string()))
                }
            })
            .ok_or(TenantCodeRejection::NoTenantCode)?
    }
}
