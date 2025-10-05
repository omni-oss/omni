use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::utils::path::escape_path_component;

#[allow(unused)]
#[derive(Deserialize, IntoParams, Debug, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct CommonArtifactQuery {
    pub org: String,
    pub ws: String,
    pub env: String,
}

#[allow(unused)]
#[inline(always)]
pub fn container_common(query: &CommonArtifactQuery) -> String {
    container(&query.org, &query.ws, &query.env)
}

#[inline(always)]
pub fn container(org: &str, ws: &str, env: &str) -> String {
    let org = escape_path_component(org);
    let ws = escape_path_component(ws);
    let env = escape_path_component(env);
    format!("{}/{}/{}", org, ws, env)
}
