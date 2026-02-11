use std::borrow::Cow;

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
    let listen = if state.args.listen.contains("0.0.0.0") {
        Cow::Owned(state.args.listen.replace("0.0.0.0", "localhost"))
    } else {
        Cow::Borrowed(&state.args.listen)
    };
    let protocol = if state.args.secure { "https" } else { "http" };

    let json = serde_json::to_string_pretty(&ScalarOptions {
        servers: Some(vec![ScalarServer {
            url: format!("{protocol}://{listen}"),
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
