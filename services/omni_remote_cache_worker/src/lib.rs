#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use omni_remote_cache_core::{
    caching::CachingState,
    config::Configuration,
    providers::{ConfigBasedDependencyProvider, DependencyProvider},
    routes::root::{RouterConfig, build_router},
    state::{ServiceInfo, ServiceState},
};
use omni_remote_cache_repository_d1::{D1Executor, WaitUntilBackground};
use omni_remote_cache_repository_query::RepositoryConfiguration;
use omni_remote_cache_storage_r2::R2Backend;
use tower_service::Service as _;
use worker::{Context, Env, HttpRequest, Result, event};

const BUCKET_BINDING: &str = "CACHE_BUCKET";
const CONFIG_BINDING: &str = "ORCS_CONFIG";
const REPOSITORY_CONFIG_BINDING: &str = "ORCS_REPOSITORY_CONFIG";
const D1_BINDING: &str = "DB";
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

/// Reads ownership data and API keys from D1 with the operator's queries
/// when `ORCS_REPOSITORY_CONFIG` is set, otherwise from `ORCS_CONFIG`.
fn build_provider(
    env: &Env,
    ctx: Context,
) -> Result<Arc<dyn DependencyProvider>> {
    if let Some(repo) = text_binding(env, REPOSITORY_CONFIG_BINDING) {
        let config =
            RepositoryConfiguration::from_json(&repo).map_err(internal)?;
        let state = caching_state(&config);
        let executor = D1Executor::new(env.d1(D1_BINDING)?);
        return config
            .build_provider(
                Arc::new(executor),
                Some(Arc::new(WaitUntilBackground::new(ctx))),
                state,
            )
            .map_err(internal);
    }

    let config = text_binding(env, CONFIG_BINDING).ok_or_else(|| {
        internal(format!("missing `{CONFIG_BINDING}` secret or variable"))
    })?;
    let config = Configuration::from_inline(config).map_err(internal)?;
    Ok(Arc::new(ConfigBasedDependencyProvider::new(
        Arc::new(config.to_in_memory_database()),
        Arc::new(config.security.api_keys.clone()),
    )))
}

thread_local! {
    // Workers run one thread per isolate. The caches live as long as the
    // isolate so cached lookups are shared by requests, unlike the provider,
    // which holds request-scoped handles and is rebuilt each time.
    static CACHING_STATE: std::cell::OnceCell<Arc<CachingState>> =
        const { std::cell::OnceCell::new() };
}

fn caching_state(
    config: &RepositoryConfiguration,
) -> Option<Arc<CachingState>> {
    let caching = config.caching.as_ref()?;
    Some(CACHING_STATE.with(|c| {
        c.get_or_init(|| Arc::new(CachingState::new(caching)))
            .clone()
    }))
}

fn build_state(
    env: &Env,
    ctx: Context,
    req: &HttpRequest,
    api_prefix: Option<String>,
) -> Result<ServiceState> {
    let provider = build_provider(env, ctx)?;

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
        provider,
    ))
}

#[event(fetch)]
async fn fetch(
    req: HttpRequest,
    env: Env,
    ctx: Context,
) -> Result<http::Response<axum::body::Body>> {
    console_error_panic_hook::set_once();

    let api_prefix = text_binding(&env, "API_PREFIX");
    let router_config = RouterConfig::new(
        api_prefix.clone(),
        bool_binding(&env, "ENABLE_OPENAPI"),
    );
    let state = build_state(&env, ctx, &req, api_prefix)?;

    let mut router = build_router(&router_config).with_state(state);

    router.call(req).await.map_err(internal)
}
