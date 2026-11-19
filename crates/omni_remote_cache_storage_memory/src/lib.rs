#![allow(clippy::redundant_field_names)]

use std::{
    collections::BTreeSet,
    sync::{Mutex, MutexGuard, PoisonError},
};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use bytesize::ByteSize;
use derive_new::new;
use futures::StreamExt as _;
use maps::{Map, UnorderedMap};
use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, ListItem, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream, error::Error,
};

#[derive(Debug, new)]
pub struct InMemoryBackend {
    #[new(default)]
    containers: Mutex<UnorderedMap<String, Map<String, Vec<u8>>>>,
}

impl InMemoryBackend {
    /// Every object whose full path starts with `prefix`, as (key relative to
    /// the prefix, size).
    fn matching(&self, prefix: &ContainerPrefix) -> Vec<(String, u64)> {
        let prefix = prefix.as_str();
        let containers = self.lock();
        let mut out = Vec::new();
        for (container, entries) in containers.iter() {
            for (key, value) in entries.iter() {
                if let Some(rel) =
                    format!("{container}/{key}").strip_prefix(prefix)
                {
                    out.push((rel.to_string(), value.len() as u64));
                }
            }
        }
        out
    }

    /// Check and insert under one lock acquisition, so concurrent creators
    /// agree on exactly one winner.
    fn insert_if_absent(
        &self,
        container: &str,
        key: &str,
        value: Vec<u8>,
    ) -> SaveOutcome {
        let mut containers = self.lock();
        let entries = containers.entry(container.to_string()).or_default();
        if entries.contains_key(key) {
            return SaveOutcome::AlreadyExists;
        }
        entries.insert(key.to_string(), value);
        SaveOutcome::Stored
    }

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

    async fn get_range(
        &self,
        container: &Container,
        key: &str,
        range: ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        Ok(self
            .lock()
            .get(container.as_str())
            .and_then(|m| m.get(key))
            .map(|v| {
                let selected = range.resolve(v.len() as u64);
                Bytes::copy_from_slice(
                    &v[selected.start as usize..selected.end as usize],
                )
            }))
    }

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        // One scan, rather than a scan per page of the provided default.
        let mut matching = self.matching(&container.prefix());
        matching.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(matching
            .into_iter()
            .map(|(key, len)| ListItem {
                key,
                size: ByteSize::b(len),
            })
            .collect())
    }

    async fn list_prefix(
        &self,
        prefix: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, Error> {
        let mut matching = self.matching(prefix);
        matching.sort_by(|a, b| a.0.cmp(&b.0));

        let limit = limit.max(1) as usize;
        let remaining: Vec<ListItem> = matching
            .into_iter()
            .filter(|(key, _)| after.is_none_or(|after| key.as_str() > after))
            .map(|(key, len)| ListItem {
                key,
                size: ByteSize::b(len),
            })
            .collect();
        let more = remaining.len() > limit;
        let items: Vec<ListItem> = remaining.into_iter().take(limit).collect();
        let next_after = if more {
            items.last().map(|item| item.key.clone())
        } else {
            None
        };

        Ok(PrefixPage { items, next_after })
    }

    async fn list_children(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        let children: BTreeSet<String> = self
            .matching(prefix)
            .into_iter()
            .filter_map(|(key, _)| {
                key.split_once('/').map(|(first, _)| first.to_string())
            })
            .collect();

        Ok(children.into_iter().collect())
    }

    async fn delete_prefix(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<u64, Error> {
        let mut containers = self.lock();
        let mut removed = 0u64;
        for (container, entries) in containers.iter_mut() {
            let doomed: Vec<String> = entries
                .keys()
                .filter(|key| {
                    format!("{container}/{key}").starts_with(prefix.as_str())
                })
                .cloned()
                .collect();
            for key in &doomed {
                entries.swap_remove(key);
            }
            removed += doomed.len() as u64;
        }
        containers.retain(|_, entries| !entries.is_empty());

        Ok(removed)
    }

    async fn save_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<SaveOutcome, Error> {
        Ok(self.insert_if_absent(container.as_str(), key, value.to_vec()))
    }

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        mut value: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        let mut combined = BytesMut::new();

        while let Some(chunk) = value.next().await {
            combined.extend_from_slice(&chunk?);
        }

        Ok(self.insert_if_absent(container.as_str(), key, combined.to_vec()))
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
