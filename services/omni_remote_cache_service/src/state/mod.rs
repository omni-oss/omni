use std::sync::Arc;

use derive_new::new;

use crate::{args::ServeArgs, storage_backend::StorageBackend};

#[derive(Debug, Clone, new)]
pub struct ServiceState {
    pub storage_backend: Arc<StorageBackend>,
    pub config: ServeArgs,
}

impl ServiceState {
    pub async fn from_args(args: &ServeArgs) -> Self {
        Self {
            storage_backend: Arc::new(
                StorageBackend::from_cli_args(args).await,
            ),
            config: args.clone(),
        }
    }
}
