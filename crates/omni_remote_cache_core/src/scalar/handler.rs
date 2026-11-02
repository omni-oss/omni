use axum::{extract::State, response::Html};
use axum_extra::{response::InternalServerError, routing::TypedPath};
use serde::{Deserialize, Serialize};

use crate::{
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
    let base_url = state.info.base_url();

    let json = serde_json::to_string_pretty(&ScalarOptions {
        servers: Some(vec![ScalarServer {
            url: base_url,
            ..Default::default()
        }]),
        ..Default::default()
    })
    .map_err(InternalServerError)?;

    let rendered = HTML_DOC
        .replace("{{documentPath}}", &doc_uri)
        .replace("{ configurationJson: null }", &json)
        .replace("{{title}}", &state.info.name);

    Ok(Html(rendered))
}

const HTML_DOC: &str = include_str!("./scalar.html");
