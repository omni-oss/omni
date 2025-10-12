use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use bytesize::ByteSize;
use derive_new::new;
use maps::{Map, UnorderedMap};
use tokio::sync::Mutex;
use tokio_stream::StreamExt as _;

use crate::{
    BoxStream, ListItem, PageOptions, RemoteCacheStorageBackend, error::Error,
};

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

    async fn exists(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<bool, Error> {
        let container = self.container(container);

        Ok(self
            .containers
            .lock()
            .await
            .get(container)
            .and_then(|m| m.get(key))
            .is_some())
    }

    async fn get_stream(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<BoxStream<Bytes>>, Error> {
        let container = self.container(container);
        let result = self
            .containers
            .lock()
            .await
            .get(container)
            .and_then(|m| m.get(key))
            .map(|v| {
                Box::pin(tokio_stream::once(Bytes::copy_from_slice(v)))
                    as BoxStream<Bytes>
            });

        Ok(result)
    }

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<Vec<ListItem>, Error> {
        let container = self.container(container);
        let len = self
            .containers
            .lock()
            .await
            .get(container)
            .map(|m| m.len())
            .unwrap_or_default();

        let paged_result = self
            .paged_list(
                Some(container),
                PageOptions::new(None, Some(len as u32)),
            )
            .await?;

        Ok(paged_result)
    }

    async fn paged_list(
        &self,
        container: Option<&str>,
        query: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        let container = self.container(container);
        let per_page = query.per_page.unwrap_or(100);
        let items = self
            .containers
            .lock()
            .await
            .get(container)
            .map(|m| {
                let position = m
                    .iter()
                    .position(|(k, _)| {
                        if let Some(after_key) = query.after_key.as_ref() {
                            *k == *after_key
                        } else {
                            true
                        }
                    })
                    .unwrap_or(0);

                m.iter()
                    .skip(position)
                    .map(|(k, v)| ListItem {
                        key: k.clone(),
                        size: ByteSize::b(v.len() as u64),
                    })
                    .take(per_page as usize)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(items)
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

    async fn save_stream(
        &self,
        container: Option<&str>,
        key: &str,
        mut value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
        let container = self.container(container);
        let mut combined = BytesMut::new();

        while let Some(chunk) = value.next().await {
            combined.extend_from_slice(&chunk);
        }

        self.containers
            .lock()
            .await
            .entry(container.to_string())
            .or_default()
            .insert(key.to_string(), combined.to_vec());

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
