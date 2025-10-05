use axum::{extract::State, response::Html};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use serde::{Deserialize, Serialize};

use crate::{
    build,
    scalar::options::{ScalarOptions, ScalarServer},
    state::ServiceState,
};

#[derive(TypedPath, Serialize, Deserialize)]
#[typed_path("/{version}/{format}/scalar")]
pub struct GetScalarUiPath {
    version: String,
    format: String,
}

#[tracing::instrument(skip(state))]
pub async fn get_scalar_ui(
    GetScalarUiPath { version, format }: GetScalarUiPath,
    State(state): State<ServiceState>,
) -> Result<Html<String>, InternalServerError<serde_json::Error>> {
    let doc_uri = format!("/openapi/{version}/{format}");
    let prefix_path = if let Some(prefix) = state
        .args
        .routes
        .as_ref()
        .map(|r| r.api_prefix.as_deref().unwrap_or("/api"))
    {
        format!("{prefix}/{version}")
    } else {
        version.clone()
    };

    let json = serde_json::to_string_pretty(&ScalarOptions {
        servers: Some(vec![ScalarServer {
            url: prefix_path,
            ..Default::default()
        }]),
        ..Default::default()
    })
    .map_err(InternalServerError)?;

    let rendered = HTML_DOC
        .replace("{{documentPath}}", &doc_uri)
        .replace("{ configurationJson: null }", &json)
        .replace("{{title}}", build::PROJECT_NAME);

    Ok(Html(rendered))
}

const HTML_DOC: &'static str = include_str!("./scalar.html");
