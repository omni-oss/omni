use std::sync::Arc;

use derive_new::new;
use omni_remote_cache_storage_core::RemoteCacheStorageBackend;

use crate::providers::DependencyProvider;

pub type DynStorageBackend = dyn RemoteCacheStorageBackend + Send + Sync;

#[derive(Debug, Clone, new)]
pub struct ServiceInfo {
    #[new(into)]
    pub name: String,
    #[new(into)]
    pub version: String,
    #[new(into)]
    pub listen: String,
    pub secure: bool,
    pub api_prefix: Option<String>,
}

impl ServiceInfo {
    pub fn api_prefix(&self) -> &str {
        self.api_prefix.as_deref().unwrap_or("/api")
    }

    pub fn base_url(&self) -> String {
        let listen = self.listen.replace("0.0.0.0", "localhost");
        let protocol = if self.secure { "https" } else { "http" };

        format!("{protocol}://{listen}")
    }
}

#[derive(new, Clone)]
pub struct ServiceState {
    pub storage_backend: Arc<DynStorageBackend>,
    pub info: Arc<ServiceInfo>,
    pub provider: Arc<dyn DependencyProvider>,
}
