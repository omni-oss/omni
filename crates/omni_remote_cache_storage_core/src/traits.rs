use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use serde::{Deserialize, Serialize};

use crate::{Container, ListItem, error::Error};

#[derive(Serialize, Deserialize, new)]
pub struct PageOptions {
    pub after_key: Option<String>,
    pub per_page: Option<u32>,
}

pub type BoxStream<T> =
    Pin<Box<dyn futures::Stream<Item = T> + 'static + Send>>;

/// Upload chunks are fallible. A backend must abort on any error and must
/// not expose bytes until the entire stream has completed successfully.
pub type WriteStream = BoxStream<Result<Bytes, Error>>;

/// Result of a create-only write. There is no replace: an existing entry is
/// kept intact and reported, so an immutable entry cannot be overwritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Stored,
    AlreadyExists,
}

/// Read side of an object: items are fallible so a mid-stream failure is
/// surfaced to the caller instead of silently truncating the data.
pub type ReadStream =
    Pin<Box<dyn futures::Stream<Item = Result<Bytes, Error>> + 'static + Send>>;

#[async_trait]
pub trait RemoteCacheStorageBackend {
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<Bytes>, Error>;

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, Error>;

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error>;

    async fn list(&self, container: &Container)
    -> Result<Vec<ListItem>, Error>;

    async fn paged_list(
        &self,
        container: &Container,
        page_options: PageOptions,
    ) -> Result<Vec<ListItem>, Error>;

    async fn save_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<SaveOutcome, Error>;

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: WriteStream,
    ) -> Result<SaveOutcome, Error>;

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), Error>;

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ByteSize>, Error>;
}
