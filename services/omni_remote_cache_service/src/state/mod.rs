use std::sync::Arc;

use derive_new::new;

use crate::{
    args::ServeArgs,
    data_impl::in_memory::InMemoryDatabase,
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
        Self {
            storage_backend: Arc::new(
                StorageBackend::from_cli_args(args).await,
            ),
            args: Arc::new(args.clone()),
            provider: Arc::new(InMemoryDependencyProvider::new(Arc::new(
                InMemoryDatabase::default(),
            ))),
        }
    }
}
