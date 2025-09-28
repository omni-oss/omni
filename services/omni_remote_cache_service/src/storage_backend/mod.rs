use std::num::NonZeroUsize;

use async_trait::async_trait;
use derive_new::new;
use omni_remote_cache_storage::{
    RemoteCacheStorageBackend,
    decorators::LruCached,
    impls::{InMemoryBackend, LocalDiskCacheBackend, S3CacheBackend},
};

use crate::args::ServeArgs;

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
        if let Some(lru_cache_cap) = args.lru_cache_capacity {
            match args.backend {
                crate::args::BackendType::S3 => {
                    let s3 = args.s3.clone().expect("s3 config is required");
                    StorageBackend::LruCachedS3(LruCached::new(
                        S3CacheBackend::from_basic_config(
                            &s3.into_basig_config(),
                        )
                        .await,
                        NonZeroUsize::new(lru_cache_cap).unwrap(),
                    ))
                }
                crate::args::BackendType::LocalDisk => {
                    let local_disk = args
                        .local_disk
                        .clone()
                        .expect("local disk path is required");
                    StorageBackend::LruCachedLocalDisk(LruCached::new(
                        LocalDiskCacheBackend::new(
                            local_disk.root_dir,
                            "default",
                        ),
                        NonZeroUsize::new(lru_cache_cap).unwrap(),
                    ))
                }
                crate::args::BackendType::InMemory => {
                    StorageBackend::InMemory(InMemoryBackend::new("default"))
                }
            }
        } else {
            match args.backend {
                crate::args::BackendType::S3 => {
                    let s3 = args.s3.clone().expect("s3 config is required");
                    StorageBackend::S3(
                        S3CacheBackend::from_basic_config(
                            &s3.into_basig_config(),
                        )
                        .await,
                    )
                }
                crate::args::BackendType::LocalDisk => {
                    let local_disk = args
                        .local_disk
                        .clone()
                        .expect("local disk path is required");
                    StorageBackend::LocalDisk(LocalDiskCacheBackend::new(
                        local_disk.root_dir,
                        local_disk.default_subdir,
                    ))
                }
                crate::args::BackendType::InMemory => {
                    StorageBackend::InMemory(InMemoryBackend::new("default"))
                }
            }
        }
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for StorageBackend {
    fn default_container(&self) -> &str {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.default_container()
            }
            StorageBackend::LocalDisk(inner) => inner.default_container(),
            StorageBackend::LruCachedS3(inner) => inner.default_container(),
            StorageBackend::S3(inner) => inner.default_container(),
            StorageBackend::InMemory(inner) => inner.default_container(),
        }
    }

    async fn get(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<bytes::Bytes>, omni_remote_cache_storage::error::Error>
    {
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

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<
        Vec<omni_remote_cache_storage::ListItem>,
        omni_remote_cache_storage::error::Error,
    > {
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

    async fn save(
        &self,
        container: Option<&str>,
        key: &str,
        value: bytes::Bytes,
    ) -> Result<(), omni_remote_cache_storage::error::Error> {
        match self {
            StorageBackend::LruCachedLocalDisk(inner) => {
                inner.save(container, key, value).await
            }
            StorageBackend::LocalDisk(inner) => {
                inner.save(container, key, value).await
            }
            StorageBackend::LruCachedS3(inner) => {
                inner.save(container, key, value).await
            }
            StorageBackend::S3(inner) => {
                inner.save(container, key, value).await
            }
            StorageBackend::InMemory(inner) => {
                inner.save(container, key, value).await
            }
        }
    }

    async fn delete(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<(), omni_remote_cache_storage::error::Error> {
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
        container: Option<&str>,
        key: &str,
    ) -> Result<
        Option<bytesize::ByteSize>,
        omni_remote_cache_storage::error::Error,
    > {
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
