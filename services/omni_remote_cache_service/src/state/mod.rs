use std::sync::Arc;

use derive_new::new;

use crate::{
    args::ServeArgs,
    config::Configuration,
    providers::{DependencyProvider, InMemoryDependencyProvider},
    storage_backend::StorageBackend,
};

#[derive(new, Clone)]
pub struct ServiceState {
    pub storage_backend: Arc<StorageBackend>,
    pub args: Arc<ServeArgs>,
    pub provider: Arc<dyn DependencyProvider>,
}

impl ServiceState {
    pub async fn from_args(args: &ServeArgs) -> Self {
        let path = args.config.as_deref().unwrap_or("orcs.config.json");

        let config = Configuration::from_file(path).unwrap();

        Self {
            storage_backend: Arc::new(
                StorageBackend::from_cli_args(args).await,
            ),
            args: Arc::new(args.clone()),
            provider: Arc::new(InMemoryDependencyProvider::new(Arc::new(
                config.to_in_memory_database(),
            ))),
        }
    }
}
