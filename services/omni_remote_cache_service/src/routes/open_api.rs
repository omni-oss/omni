use std::{borrow::Cow, collections::BTreeMap};

use axum::{Json, Router, extract::State, response::IntoResponse};
use axum_extra::routing::{RouterExt, TypedPath};
use serde::{Deserialize, Serialize};
use strum::Display;
use utoipa::{
    OpenApi as _,
    openapi::{OpenApi, PathItem, Server},
};

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

fn apply(
    api_prefix: &str,
    addr: &str,
    version: Version,
    mut openapi: OpenApi,
) -> OpenApi {
    let api_prefix = api_prefix.trim();
    let base_path = if !api_prefix.is_empty() {
        format!("{api_prefix}/{version}")
    } else {
        format!("{version}")
    };

    openapi.paths.paths = openapi
        .paths
        .paths
        .iter()
        .map(|(k, v)| (format!("{base_path}{k}"), v.clone()))
        .collect::<BTreeMap<String, PathItem>>();

    if let Some(servers) = openapi.servers.as_mut() {
        servers.iter_mut().for_each(|s| {
            s.url = s.url.replace(&base_path, "");
        });
    } else {
        openapi.servers = Some(vec![Server::new(addr)]);
    }

    openapi
}

#[tracing::instrument(skip(version, format, state))]
pub async fn get_open_api_doc(
    GetOpenApiDocs { version, format }: GetOpenApiDocs,
    State(state): State<ServiceState>,
) -> axum::response::Response {
    let api_prefix = state
        .args
        .routes
        .as_ref()
        .and_then(|r| r.api_prefix.as_deref())
        .unwrap_or("/api");
    let listen = if state.args.listen.contains("0.0.0.0") {
        Cow::Owned(state.args.listen.replace("0.0.0.0", "localhost"))
    } else {
        Cow::Borrowed(&state.args.listen)
    };
    let protocol = if state.args.secure { "https" } else { "http" };
    let addr = format!("{protocol}://{listen}");

    match format {
        Format::Json => match version {
            Version::V1 => {
                Json(apply(api_prefix, &addr, version, V1RootApiDoc::openapi()))
                    .into_response()
            }
        },
        Format::Yaml => match version {
            Version::V1 => {
                Yaml(apply(api_prefix, &addr, version, V1RootApiDoc::openapi()))
                    .into_response()
            }
        },
    }
}
