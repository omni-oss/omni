#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use omni_remote_cache_core::{
    config::Configuration,
    providers::ConfigBasedDependencyProvider,
    routes::root::{RouterConfig, build_router},
    state::{ServiceInfo, ServiceState},
};
use omni_remote_cache_storage_r2::R2Backend;
use tower_service::Service as _;
use worker::{Context, Env, HttpRequest, Result, event};

const BUCKET_BINDING: &str = "CACHE_BUCKET";
const CONFIG_BINDING: &str = "ORCS_CONFIG";
const DEFAULT_CONTAINER: &str = "default";

fn text_binding(env: &Env, name: &str) -> Option<String> {
    env.secret(name)
        .map(|s| s.to_string())
        .or_else(|_| env.var(name).map(|v| v.to_string()))
        .ok()
}

fn bool_binding(env: &Env, name: &str) -> bool {
    text_binding(env, name).is_some_and(|v| v == "true" || v == "1")
}

fn internal(e: impl std::fmt::Display) -> worker::Error {
    worker::Error::RustError(e.to_string())
}

fn build_state(
    env: &Env,
    req: &HttpRequest,
    api_prefix: Option<String>,
) -> Result<ServiceState> {
    let config = text_binding(env, CONFIG_BINDING).ok_or_else(|| {
        internal(format!("missing `{CONFIG_BINDING}` secret or variable"))
    })?;
    let config = Configuration::from_inline(config).map_err(internal)?;

    let bucket = env.bucket(BUCKET_BINDING)?;
    let container = text_binding(env, "DEFAULT_CONTAINER")
        .unwrap_or_else(|| DEFAULT_CONTAINER.to_string());

    let uri = req.uri();
    let info = ServiceInfo::new(
        "omni-remote-cache-worker",
        env!("CARGO_PKG_VERSION"),
        uri.authority().map(|a| a.as_str()).unwrap_or_default(),
        uri.scheme_str() == Some("https"),
        api_prefix,
    );

    Ok(ServiceState::new(
        Arc::new(R2Backend::new(bucket, container)),
        Arc::new(info),
        Arc::new(ConfigBasedDependencyProvider::new(
            Arc::new(config.to_in_memory_database()),
            Arc::new(config.security.api_keys.clone()),
        )),
    ))
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<http::Response<axum::body::Body>> {
    console_error_panic_hook::set_once();

    let api_prefix = text_binding(&env, "API_PREFIX");
    let router_config = RouterConfig::new(
        api_prefix.clone(),
        bool_binding(&env, "ENABLE_OPENAPI"),
    );
    let state = build_state(&env, &req, api_prefix)?;

    let mut router = build_router(&router_config).with_state(state);

    router.call(req).await.map_err(internal)
}
