use std::sync::Arc;

use derive_new::new;

use crate::{
    args::{ConfigType, ServeArgs},
    config::Configuration,
    providers::{ConfigBasedDependencyProvider, DependencyProvider},
    storage_backend::StorageBackend,
};

#[derive(new, Clone)]
pub struct ServiceState {
    pub storage_backend: Arc<StorageBackend>,
    pub args: Arc<ServeArgs>,
    pub provider: Arc<dyn DependencyProvider>,
}

impl ServiceState {
    pub async fn from_args(args: &ServeArgs) -> eyre::Result<Self> {
        let config = args.config.as_deref().unwrap_or("orcs.config.json");
        let cfg_type = args.config_type.unwrap_or(ConfigType::File);

        let config = match cfg_type {
            ConfigType::Inline => Configuration::from_inline(config)?,
            ConfigType::File => Configuration::from_file(config)?,
        };

        Ok(Self {
            storage_backend: Arc::new(
                StorageBackend::from_cli_args(args).await,
            ),
            args: Arc::new(args.clone()),
            provider: Arc::new(ConfigBasedDependencyProvider::new(
                Arc::new(config.to_in_memory_database()),
                Arc::new(config.security.api_keys.clone()),
            )),
        })
    }
}
