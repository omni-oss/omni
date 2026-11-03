use axum::Router;
use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::{
    routes::{open_api, v1},
    state::ServiceState,
};

#[derive(Default, Serialize, Deserialize, Debug, Clone, new)]
pub struct RouterConfig {
    pub api_prefix: Option<String>,
    pub enable_openapi: bool,
}

pub fn build_router(conf: &RouterConfig) -> Router<ServiceState> {
    let api = Router::new().nest("/v1", v1::root::build_router());

    let router =
        Router::new().nest(conf.api_prefix.as_deref().unwrap_or("/api"), api);

    if conf.enable_openapi {
        router.nest("/openapi", open_api::build_router())
    } else {
        router
    }
}
