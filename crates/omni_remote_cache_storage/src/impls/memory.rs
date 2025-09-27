use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use maps::{Map, UnorderedMap};
use tokio::sync::Mutex;

use crate::{ListItem, RemoteCacheStorageBackend, error::Error};

#[derive(Debug, new)]
pub struct InMemoryBackend {
    #[new(default)]
    containers: Mutex<UnorderedMap<String, Map<String, Vec<u8>>>>,
    #[new(into)]
    default_container: String,
}

impl InMemoryBackend {
    fn container<'s>(&'s self, container: Option<&'s str>) -> &'s str {
        container.unwrap_or(&self.default_container)
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for InMemoryBackend {
    fn default_container(&self) -> &str {
        &self.default_container
    }

    async fn get(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        let container = self.container(container);

        Ok(self
            .containers
            .lock()
            .await
            .get(container)
            .and_then(|m| m.get(key))
            .map(|v| Bytes::copy_from_slice(v)))
    }

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<Vec<ListItem>, Error> {
        let container = self.container(container);
        Ok(self
            .containers
            .lock()
            .await
            .get(container)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| ListItem {
                        key: k.clone(),
                        size: ByteSize::b(v.len() as u64),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn save(
        &self,
        container: Option<&str>,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error> {
        let container = self.container(container);
        self.containers
            .lock()
            .await
            .entry(container.to_string())
            .or_default()
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }

    async fn delete(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<(), Error> {
        let container = self.container(container);

        self.containers
            .lock()
            .await
            .get_mut(container)
            .and_then(|m| m.swap_remove(key));

        Ok(())
    }

    async fn size(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let container = self.container(container);

        Ok(self
            .containers
            .lock()
            .await
            .get(container)
            .and_then(|m| m.get(key))
            .map(|v| ByteSize::b(v.len() as u64)))
    }
}

#[cfg(test)]
mod tests {
    use crate::decl_remote_cache_storage_backend_tests;

    decl_remote_cache_storage_backend_tests!(super::InMemoryBackend::new(
        "default".to_string()
    ));
}
