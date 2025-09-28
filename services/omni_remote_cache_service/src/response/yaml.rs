use axum::response::IntoResponse;
use axum_extra::response::InternalServerError;
use bytes::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Yaml<T>(pub T);

impl<T: Serialize> IntoResponse for Yaml<T> {
    fn into_response(self) -> axum::response::Response {
        let yaml = serde_norway::to_string(&self.0);

        match yaml {
            Ok(d) => (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/yaml")],
                Bytes::from(d),
            )
                .into_response(),
            Err(e) => InternalServerError(e).into_response(),
        }
    }
}
