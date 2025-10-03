use axum::response::Html;
use axum_extra::{response::InternalServerError, routing::TypedPath};
use serde::{Deserialize, Serialize};

use crate::scalar::options::ScalarOptions;

#[derive(TypedPath, Serialize, Deserialize)]
#[typed_path("/{version}/{format}/scalar")]
pub struct GetScalarUiPath {
    version: String,
    format: String,
}

pub async fn get_scalar_ui(
    GetScalarUiPath { version, format }: GetScalarUiPath,
) -> Result<Html<String>, InternalServerError<serde_json::Error>> {
    let doc_uri = format!("/openapi/{version}/{format}");
    let json = serde_json::to_string_pretty(&ScalarOptions::default())
        .map_err(InternalServerError)?;
    let rendered = HTML_DOC
        .replace("{{documentPath}}", &doc_uri)
        .replace("{ configurationJson: null }", &json);

    Ok(Html(rendered))
}

const HTML_DOC: &'static str = include_str!("./scalar.html");
