use axum::Router;
use clap::ArgAction;
use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::{
    routes::{open_api, v1},
    state::ServiceState,
};

#[derive(Default, Serialize, Deserialize, clap::Args, Debug, Clone, new)]
pub struct RouterConfig {
    #[clap(
        long = "routes.api-prefix",
        env = "OMNI_REMOTE_CACHE_SERVICE_ROUTES_API_PREFIX",
        default_value = "/api"
    )]
    pub api_prefix: Option<String>,

    #[clap(
        long = "routes.enable-openapi",
        env = "OMNI_REMOTE_CACHE_SERVICE_ROUTES_ENABLE_OPENAPI",
        action = ArgAction::SetTrue,
        default_value = "false"
    )]
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
