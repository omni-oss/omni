#![allow(clippy::redundant_field_names)]

use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use bytesize::ByteSize;
use derive_new::new;
use futures::StreamExt as _;
use maps::{Map, UnorderedMap};
use omni_remote_cache_storage_core::{
    BoxStream, Container, ListItem, PageOptions, ReadStream,
    RemoteCacheStorageBackend, error::Error,
};

#[derive(Debug, new)]
pub struct InMemoryBackend {
    #[new(default)]
    containers: Mutex<UnorderedMap<String, Map<String, Vec<u8>>>>,
}

impl InMemoryBackend {
    fn lock(
        &self,
    ) -> MutexGuard<'_, UnorderedMap<String, Map<String, Vec<u8>>>> {
        self.containers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for InMemoryBackend {
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        let container = container.as_str();

        Ok(self
            .lock()
            .get(container)
            .and_then(|m| m.get(key))
            .map(|v| Bytes::copy_from_slice(v)))
    }

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, Error> {
        let container = container.as_str();

        Ok(self
            .lock()
            .get(container)
            .and_then(|m| m.get(key))
            .is_some())
    }

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error> {
        let container = container.as_str();
        let result =
            self.lock()
                .get(container)
                .and_then(|m| m.get(key))
                .map(|v| {
                    Box::pin(futures::stream::once(std::future::ready(Ok(
                        Bytes::copy_from_slice(v),
                    )))) as ReadStream
                });

        Ok(result)
    }

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        let len = self
            .lock()
            .get(container.as_str())
            .map(|m| m.len())
            .unwrap_or_default();

        let paged_result = self
            .paged_list(container, PageOptions::new(None, Some(len as u32)))
            .await?;

        Ok(paged_result)
    }

    async fn paged_list(
        &self,
        container: &Container,
        query: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        let container = container.as_str();
        let per_page = query.per_page.unwrap_or(100);
        let items = self
            .lock()
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
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error> {
        let container = container.as_str();
        self.lock()
            .entry(container.to_string())
            .or_default()
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }

    async fn save_stream(
        &self,
        container: &Container,
        key: &str,
        mut value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
        let container = container.as_str();
        let mut combined = BytesMut::new();

        while let Some(chunk) = value.next().await {
            combined.extend_from_slice(&chunk);
        }

        self.lock()
            .entry(container.to_string())
            .or_default()
            .insert(key.to_string(), combined.to_vec());

        Ok(())
    }

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), Error> {
        let container = container.as_str();

        self.lock()
            .get_mut(container)
            .and_then(|m| m.swap_remove(key));

        Ok(())
    }

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let container = container.as_str();

        Ok(self
            .lock()
            .get(container)
            .and_then(|m| m.get(key))
            .map(|v| ByteSize::b(v.len() as u64)))
    }
}

#[cfg(test)]
mod tests {
    use omni_remote_cache_storage_core::decl_remote_cache_storage_backend_tests;

    decl_remote_cache_storage_backend_tests!(super::InMemoryBackend::new());
}
