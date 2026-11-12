use std::sync::Arc;

pub use omni_remote_cache_core::state::ServiceState;
use omni_remote_cache_core::{
    config::Configuration, providers::ConfigBasedDependencyProvider,
    state::ServiceInfo,
};

use crate::{
    args::{ConfigType, ServeArgs},
    build,
    repository::build_sql_provider,
    storage_backend::StorageBackend,
};

fn service_info(args: &ServeArgs) -> ServiceInfo {
    ServiceInfo::new(
        build::PROJECT_NAME,
        build::PKG_VERSION,
        args.listen.clone(),
        args.secure,
        args.routes.as_ref().and_then(|r| r.api_prefix.clone()),
    )
}

pub async fn build_state(args: &ServeArgs) -> eyre::Result<ServiceState> {
    if let Some(path) = args.repository_config.as_deref() {
        let info = service_info(args);
        let provider = build_sql_provider(path).await?;
        return Ok(ServiceState::new(
            Arc::new(StorageBackend::from_cli_args(args).await),
            Arc::new(info),
            provider,
        ));
    }

    let config = args.config.as_deref().unwrap_or("orcs.config.json");
    let cfg_type = args.config_type.unwrap_or(ConfigType::File);

    let config = match cfg_type {
        ConfigType::Inline => Configuration::from_inline(config)?,
        ConfigType::File => Configuration::from_file(config)?,
    };

    let info = service_info(args);

    Ok(ServiceState::new(
        Arc::new(StorageBackend::from_cli_args(args).await),
        Arc::new(info),
        Arc::new(ConfigBasedDependencyProvider::new(
            Arc::new(config.to_in_memory_database()),
            Arc::new(config.security.api_keys.clone()),
        )),
    ))
}
