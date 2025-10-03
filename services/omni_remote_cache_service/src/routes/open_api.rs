use axum::{Json, Router, response::IntoResponse};
use axum_extra::routing::{RouterExt, TypedPath};
use serde::{Deserialize, Serialize};
use strum::Display;
use utoipa::OpenApi;

use crate::{
    response::yaml::Yaml, routes::v1::root::V1RootApiDoc,
    scalar::handler::get_scalar_ui, state::ServiceState,
};

pub fn build_router() -> Router<ServiceState> {
    Router::new()
        .typed_get(get_open_api_doc)
        .typed_get(get_scalar_ui)
}

#[derive(TypedPath, Serialize, Deserialize)]
#[typed_path("/{version}/{format}")]
pub struct GetOpenApiDocs {
    version: Version,
    format: Format,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Display,
)]
#[serde(rename_all = "kebab-case")]
pub enum Version {
    #[strum(serialize = "v1")]
    V1,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Display,
)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    #[strum(serialize = "json")]
    Json,
    #[strum(serialize = "yaml")]
    Yaml,
}

pub async fn get_open_api_doc(
    GetOpenApiDocs { version, format }: GetOpenApiDocs,
) -> axum::response::Response {
    match format {
        Format::Json => match version {
            Version::V1 => Json(V1RootApiDoc::openapi()).into_response(),
        },
        Format::Yaml => match version {
            Version::V1 => Yaml(V1RootApiDoc::openapi()).into_response(),
        },
    }
}
