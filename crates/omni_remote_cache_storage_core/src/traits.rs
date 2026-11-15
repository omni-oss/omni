use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use bytesize::ByteSize;

use crate::{Container, ContainerPrefix, ListItem, error::Error};

pub type BoxStream<T> =
    Pin<Box<dyn futures::Stream<Item = T> + 'static + Send>>;

/// Upload chunks are fallible. A backend must abort on any error and must
/// not expose bytes until the entire stream has completed successfully.
pub type WriteStream = BoxStream<Result<Bytes, Error>>;

/// Page size [`RemoteCacheStorageBackend::list`] asks for. S3 and R2 return at
/// most 1000 keys per request.
pub const LIST_PAGE_SIZE: u32 = 1000;

/// A byte range to read from one object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteRange {
    /// The last `n` bytes (S3 `Range: bytes=-n`).
    Suffix(u64),
    /// Bytes `start..end` (end exclusive).
    Bounded(u64, u64),
}

impl ByteRange {
    /// The bytes this range selects from an object of `len` bytes, as
    /// `start..end` clamped to the object. Empty when nothing is selected.
    pub fn resolve(self, len: u64) -> std::ops::Range<u64> {
        let (start, end) = match self {
            ByteRange::Suffix(n) => (len.saturating_sub(n), len),
            ByteRange::Bounded(start, end) => (start.min(len), end.min(len)),
        };
        if start >= end { 0..0 } else { start..end }
    }
}

/// One page of [`RemoteCacheStorageBackend::list_prefix`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrefixPage {
    pub items: Vec<ListItem>,
    /// The cursor for the next page, or `None` when the listing is complete.
    pub next_after: Option<String>,
}

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

    /// Reads part of an object. `Ok(None)` means the object does not exist; a
    /// range that selects no bytes (`Suffix(0)`, an empty or inverted
    /// `Bounded`, or a start at or past the end) is `Ok(Some(empty))`, and a
    /// range that runs past the end is clamped to it.
    async fn get_range(
        &self,
        container: &Container,
        key: &str,
        range: ByteRange,
    ) -> Result<Option<Bytes>, Error>;

    /// Lists every object under `prefix`, recursively, in ascending key order.
    /// Keys are relative to the prefix and may contain `/`. `after` is an
    /// exclusive cursor (the last key of the previous page) and `limit` bounds
    /// the page; it is clamped to at least 1, and a page may be empty while
    /// `next_after` is `Some` (for example a folder marker was skipped).
    /// `PrefixPage::next_after` is `Some` exactly when more keys may follow.
    async fn list_prefix(
        &self,
        prefix: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, Error>;

    /// Names of the immediate sub-prefixes under `prefix` (the next path
    /// segment only, a delimiter listing), sorted and complete. Objects that sit
    /// directly under `prefix` are not children. A scan uses this to learn which
    /// environments or principals exist without reading every entry.
    async fn list_children(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Vec<String>, Error>;

    /// Deletes every object under `prefix` and returns how many were removed.
    /// Not atomic: on an error some objects may already be gone, and calling it
    /// again finishes the job. The trailing `/` of a [`ContainerPrefix`] means a
    /// prefix for `ci` never touches `ci2`.
    async fn delete_prefix(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<u64, Error>;

    /// Every object in `container`, assembled from [`Self::list_prefix`] pages.
    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        let prefix = container.prefix();
        let mut items = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = self
                .list_prefix(&prefix, after.as_deref(), LIST_PAGE_SIZE)
                .await?;
            items.extend(page.items);
            match page.next_after {
                Some(next) => after = Some(next),
                None => return Ok(items),
            }
        }
    }

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

#[cfg(test)]
mod tests {
    use super::ByteRange::{Bounded, Suffix};

    #[test]
    fn a_range_resolves_to_the_clamped_bytes_it_selects() {
        // (range, object length, expected start..end)
        for (range, len, want) in [
            (Suffix(3), 6, 3..6),
            (Suffix(100), 6, 0..6),
            (Suffix(0), 6, 0..0),
            (Suffix(3), 0, 0..0),
            (Bounded(1, 4), 6, 1..4),
            (Bounded(4, 100), 6, 4..6),
            (Bounded(0, 6), 6, 0..6),
            (Bounded(3, 3), 6, 0..0),
            (Bounded(4, 2), 6, 0..0),
            (Bounded(10, 12), 6, 0..0),
            (Bounded(6, 9), 6, 0..0),
            (Bounded(0, u64::MAX), 6, 0..6),
            (Suffix(u64::MAX), 6, 0..6),
        ] {
            assert_eq!(range.resolve(len), want, "{range:?} of {len}");
        }
    }
}
