use std::num::NonZeroUsize;

use async_trait::async_trait;
use bytes::Bytes;
use derive_new::new;
use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, ListItem, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream,
    decorators::LruCached,
    error::{self, Error},
};
use omni_remote_cache_storage_local::LocalDiskCacheBackend;
use omni_remote_cache_storage_memory::InMemoryBackend;
use omni_remote_cache_storage_s3::S3CacheBackend;

use crate::args::{BackendType, ServeArgs};

#[derive(Debug, new)]
pub enum StorageBackend {
    LruCachedLocalDisk(LruCached<LocalDiskCacheBackend>),
    LocalDisk(LocalDiskCacheBackend),
    LruCachedS3(LruCached<S3CacheBackend>),
    S3(S3CacheBackend),
    InMemory(InMemoryBackend),
}

impl StorageBackend {
    pub async fn from_cli_args(args: &ServeArgs) -> Self {
        if let Some(lru_cache_cap) = args.lru_cache_capacity
            && lru_cache_cap > 0
            && args.backend != BackendType::InMemory
        {
            match args.backend {
                BackendType::S3 => {
                    let s3 = args.s3.clone().expect("s3 config is required");
                    StorageBackend::LruCachedS3(LruCached::new(
                        S3CacheBackend::from_basic_config(
                            &s3.into_basig_config(),
                        )
                        .await,
                        NonZeroUsize::new(lru_cache_cap).unwrap(),
                    ))
                }
                BackendType::LocalDisk => {
                    let local_disk = args
                        .local_disk
                        .clone()
                        .expect("local disk path is required");
                    StorageBackend::LruCachedLocalDisk(LruCached::new(
                        LocalDiskCacheBackend::new(local_disk.root_dir),
                        NonZeroUsize::new(lru_cache_cap).unwrap(),
                    ))
                }
                _ => {
                    panic!(
                        "Lru Cache for backend '{}' is not supported",
                        args.backend
                    )
                }
            }
        } else {
            match args.backend {
                BackendType::S3 => {
                    let s3 = args.s3.clone().expect("s3 config is required");
                    StorageBackend::S3(
                        S3CacheBackend::from_basic_config(
                            &s3.into_basig_config(),
                        )
                        .await,
                    )
                }
                BackendType::LocalDisk => {
                    let local_disk = args
                        .local_disk
                        .clone()
                        .expect("local disk path is required");
                    StorageBackend::LocalDisk(LocalDiskCacheBackend::new(
                        local_disk.root_dir,
                    ))
                }
                BackendType::InMemory => {
                    StorageBackend::InMemory(InMemoryBackend::new())
                }
            }
        }
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for StorageBackend {
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<Bytes>, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.get(container, key).await
            }
            StorageBackend::LocalDisk(inner) => inner.get(container, key).await,
            StorageBackend::LruCachedS3(inner) => {
                inner.get(container, key).await
            }
            StorageBackend::S3(inner) => inner.get(container, key).await,
            StorageBackend::InMemory(inner) => inner.get(container, key).await,
        }
    }

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.exists(container, key).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.exists(container, key).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.exists(container, key).await
            }
            StorageBackend::S3(inner) => inner.exists(container, key).await,
            StorageBackend::InMemory(inner) => {
                inner.exists(container, key).await
            }
        }
    }

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.get_stream(container, key).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.get_stream(container, key).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.get_stream(container, key).await
            }
            StorageBackend::S3(inner) => inner.get_stream(container, key).await,
            StorageBackend::InMemory(inner) => {
                inner.get_stream(container, key).await
            }
        }
    }

    async fn get_range(
        &self,
        container: &Container,
        key: &str,
        range: ByteRange,
    ) -> Result<Option<Bytes>, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.get_range(container, key, range).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.get_range(container, key, range).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.get_range(container, key, range).await
            }
            StorageBackend::S3(inner) => {
                inner.get_range(container, key, range).await
            }
            StorageBackend::InMemory(inner) => {
                inner.get_range(container, key, range).await
            }
        }
    }

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.list(container).await
            }
            StorageBackend::LocalDisk(inner) => inner.list(container).await,
            StorageBackend::LruCachedS3(inner) => inner.list(container).await,
            StorageBackend::S3(inner) => inner.list(container).await,
            StorageBackend::InMemory(inner) => inner.list(container).await,
        }
    }

    async fn list_prefix(
        &self,
        prefix: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.list_prefix(prefix, after, limit).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.list_prefix(prefix, after, limit).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.list_prefix(prefix, after, limit).await
            }
            StorageBackend::S3(inner) => {
                inner.list_prefix(prefix, after, limit).await
            }
            StorageBackend::InMemory(inner) => {
                inner.list_prefix(prefix, after, limit).await
            }
        }
    }

    async fn list_children(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Vec<String>, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.list_children(prefix).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.list_children(prefix).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.list_children(prefix).await
            }
            StorageBackend::S3(inner) => inner.list_children(prefix).await,
            StorageBackend::InMemory(inner) => {
                inner.list_children(prefix).await
            }
        }
    }

    async fn delete_prefix(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<u64, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.delete_prefix(prefix).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.delete_prefix(prefix).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.delete_prefix(prefix).await
            }
            StorageBackend::S3(inner) => inner.delete_prefix(prefix).await,
            StorageBackend::InMemory(inner) => {
                inner.delete_prefix(prefix).await
            }
        }
    }

    async fn save_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<SaveOutcome, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.save_if_absent(container, key, value).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.save_if_absent(container, key, value).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.save_if_absent(container, key, value).await
            }
            StorageBackend::S3(inner) => {
                inner.save_if_absent(container, key, value).await
            }
            StorageBackend::InMemory(inner) => {
                inner.save_if_absent(container, key, value).await
            }
        }
    }

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.save_stream_if_absent(container, key, value).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.save_stream_if_absent(container, key, value).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.save_stream_if_absent(container, key, value).await
            }
            StorageBackend::S3(inner) => {
                inner.save_stream_if_absent(container, key, value).await
            }
            StorageBackend::InMemory(inner) => {
                inner.save_stream_if_absent(container, key, value).await
            }
        }
    }

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.delete(container, key).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.delete(container, key).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.delete(container, key).await
            }
            StorageBackend::S3(inner) => inner.delete(container, key).await,
            StorageBackend::InMemory(inner) => {
                inner.delete(container, key).await
            }
        }
    }

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<bytesize::ByteSize>, error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.size(container, key).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.size(container, key).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.size(container, key).await
            }
            StorageBackend::S3(inner) => inner.size(container, key).await,
            StorageBackend::InMemory(inner) => inner.size(container, key).await,
        }
    }
}
