#![cfg(target_arch = "wasm32")]
#![allow(clippy::redundant_field_names)]

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use bytesize::ByteSize;
use futures::{Stream, StreamExt as _};
use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, ListItem, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream, error::Error,
};
use send_wrapper::SendWrapper;
use worker::{Bucket, Conditional, Range, send::SendFuture};

const MAX_LIST_LIMIT: u32 = 1000;

/// A [`RemoteCacheStorageBackend`] backed by a Cloudflare R2 bucket binding.
///
/// A bucket is a single namespace, so containers are represented as key
/// prefixes: `{container}/{key}`.
///
/// Workers are single threaded, so the `!Send` JS handles are wrapped to
/// satisfy the `Send + Sync` bounds required by axum state.
pub struct R2Backend {
    bucket: SendWrapper<Bucket>,
}

impl R2Backend {
    pub fn new(bucket: Bucket) -> Self {
        Self {
            bucket: SendWrapper::new(bucket),
        }
    }

    fn prefix(&self, container: &Container) -> String {
        container.prefix().as_str().to_string()
    }

    fn object_key(&self, container: &Container, key: &str) -> String {
        format!("{}{}", self.prefix(container), key)
    }
}

fn err(e: worker::Error) -> Error {
    Error::custom(eyre::eyre!(e.to_string()))
}

type LocalReadStream = Pin<Box<dyn Stream<Item = Result<Bytes, Error>>>>;

struct SendStream(SendWrapper<LocalReadStream>);

impl Stream for SendStream {
    type Item = Result<Bytes, Error>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.get_mut().0.as_mut().poll_next(cx)
    }
}

/// A create-only write: one conditional put with `etagDoesNotMatch: "*"`.
///
/// Verified on real R2 (RFC 0030 section 8): of 24 concurrent writers to one key
/// exactly one stores and the others get a precondition failure (`null`). A
/// multipart upload is not used because `complete` takes no condition and
/// replaces an existing object (also observed on real R2), so the whole entry is
/// buffered and written once. The service caps every entry
/// (`max_artifact_size`, 32 MiB by default on Workers), which bounds the buffer.
async fn put_if_absent(
    bucket: &Bucket,
    key: &str,
    value: Vec<u8>,
) -> Result<SaveOutcome, Error> {
    let stored = bucket
        .put(key, value)
        .only_if(Conditional {
            etag_does_not_match: Some("*".to_string()),
            ..Default::default()
        })
        .execute()
        .await
        .map_err(err)?;
    Ok(if stored.is_some() {
        SaveOutcome::Stored
    } else {
        SaveOutcome::AlreadyExists
    })
}

/// Collects the sealed stream. An upstream error aborts before anything is
/// written, so a failed upload commits nothing.
async fn collect(mut value: WriteStream) -> Result<Vec<u8>, Error> {
    let mut buf = BytesMut::new();
    while let Some(chunk) = value.next().await {
        buf.extend_from_slice(&chunk?);
    }
    Ok(buf.to_vec())
}

/// R2 reports sizes and offsets as JavaScript numbers, which are exact only up to
/// 2^53 - 1, and the worker crate asserts on anything larger.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// One page of a recursive listing. `start_after` is a full object key.
async fn list_objects(
    bucket: &Bucket,
    prefix: &str,
    start_after: Option<String>,
    limit: u32,
) -> Result<(Vec<(String, u64)>, bool), Error> {
    let mut builder = bucket.list().prefix(prefix).limit(limit);
    if let Some(start_after) = start_after {
        builder = builder.start_after(start_after);
    }
    let objects = builder.execute().await.map_err(err)?;
    let found = objects
        .objects()
        .into_iter()
        .map(|o| (o.key(), o.size()))
        .collect();
    Ok((found, objects.truncated()))
}

#[async_trait]
impl RemoteCacheStorageBackend for R2Backend {
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            let Some(object) =
                self.bucket.get(key).execute().await.map_err(err)?
            else {
                return Ok(None);
            };
            let Some(body) = object.body() else {
                return Ok(None);
            };

            Ok(Some(Bytes::from(body.bytes().await.map_err(err)?)))
        })
        .await
    }

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            Ok(self.bucket.head(key).await.map_err(err)?.is_some())
        })
        .await
    }

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            let Some(object) =
                self.bucket.get(key).execute().await.map_err(err)?
            else {
                return Ok(None);
            };
            let Some(body) = object.body() else {
                return Ok(None);
            };

            let stream = body.stream().map_err(err)?;
            let stream = stream.map(|r| r.map(Bytes::from).map_err(err));

            Ok(Some(
                Box::pin(SendStream(SendWrapper::new(Box::pin(stream))))
                    as ReadStream,
            ))
        })
        .await
    }

    async fn get_range(
        &self,
        container: &Container,
        key: &str,
        range: ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            // Resolve against the real size first, so the request never depends
            // on how R2 treats a range that runs past the end.
            let Some(head) = self.bucket.head(&key).await.map_err(err)? else {
                return Ok(None);
            };
            let selected = range.resolve(head.size());
            if selected.is_empty() {
                return Ok(Some(Bytes::new()));
            }
            if selected.end > MAX_SAFE_INTEGER {
                return Err(Error::custom(eyre::eyre!(
                    "R2 range {selected:?} exceeds the largest exact offset"
                )));
            }
            let Some(object) = self
                .bucket
                .get(&key)
                .range(Range::OffsetWithLength {
                    offset: selected.start,
                    length: selected.end - selected.start,
                })
                .execute()
                .await
                .map_err(err)?
            else {
                // Deleted between the HEAD and the read.
                return Ok(None);
            };
            // The object exists but has no body to read: not a normal outcome.
            let body = object.body().ok_or_else(|| {
                Error::custom(eyre::eyre!("R2 returned no body for {key:?}"))
            })?;

            Ok(Some(Bytes::from(body.bytes().await.map_err(err)?)))
        })
        .await
    }

    async fn list_prefix(
        &self,
        prefix: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, Error> {
        let prefix = prefix.as_str();
        let start_after = after.map(|a| format!("{prefix}{a}"));
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        SendFuture::new(async {
            let (found, truncated) =
                list_objects(&self.bucket, prefix, start_after, limit).await?;
            let mut items = Vec::with_capacity(found.len());
            let mut last = None;
            for (key, size) in found {
                let relative = key.strip_prefix(prefix).ok_or_else(|| {
                    Error::custom(eyre::eyre!(
                        "R2 listing returned a key outside the prefix"
                    ))
                })?;
                last = Some(relative.to_string());
                // A console-created folder marker has no relative name.
                if !relative.is_empty() {
                    items.push(ListItem::new(
                        relative.to_string(),
                        ByteSize::b(size),
                    ));
                }
            }
            Ok(PrefixPage {
                items,
                next_after: if truncated { last } else { None },
            })
        })
        .await
    }

    async fn list_children(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        let prefix = prefix.as_str();
        SendFuture::new(async {
            let mut names = std::collections::BTreeSet::new();
            let mut cursor: Option<String> = None;
            loop {
                let mut builder = self
                    .bucket
                    .list()
                    .prefix(prefix)
                    .delimiter("/")
                    .limit(MAX_LIST_LIMIT);
                if let Some(cursor) = cursor.take() {
                    builder = builder.cursor(cursor);
                }
                let objects = builder.execute().await.map_err(err)?;
                for delimited in objects.delimited_prefixes() {
                    if let Some(name) = delimited
                        .strip_prefix(prefix)
                        .and_then(|rest| rest.strip_suffix('/'))
                        .filter(|name| !name.is_empty())
                    {
                        names.insert(name.to_string());
                    }
                }
                match objects.truncated().then(|| objects.cursor()).flatten() {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
            Ok(names.into_iter().collect())
        })
        .await
    }

    async fn delete_prefix(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<u64, Error> {
        let prefix = prefix.as_str();
        SendFuture::new(async {
            let mut deleted = 0u64;
            let mut previous_first: Option<String> = None;
            loop {
                // List from the start each time: deleted keys are gone (R2
                // deletes are strongly consistent), so no cursor is needed.
                let (found, _) =
                    list_objects(&self.bucket, prefix, None, MAX_LIST_LIMIT)
                        .await?;
                let Some((first, _)) = found.first() else {
                    return Ok(deleted);
                };
                // A key that survives its own delete would loop forever.
                if previous_first.as_deref() == Some(first.as_str()) {
                    return Err(Error::custom(eyre::eyre!(
                        "R2 did not delete {first:?}; stopping the purge"
                    )));
                }
                previous_first = Some(first.clone());
                let keys: Vec<String> =
                    found.into_iter().map(|(key, _)| key).collect();
                deleted += keys.len() as u64;
                self.bucket.delete_multiple(keys).await.map_err(err)?;
            }
        })
        .await
    }

    async fn save_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<SaveOutcome, Error> {
        let key = self.object_key(container, key);
        SendFuture::new(put_if_absent(&self.bucket, &key, value.to_vec())).await
    }

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            let body = collect(value).await?;
            put_if_absent(&self.bucket, &key, body).await
        })
        .await
    }

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            self.bucket.delete(key).await.map_err(err)?;

            Ok(())
        })
        .await
    }

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            Ok(self
                .bucket
                .head(key)
                .await
                .map_err(err)?
                .map(|o| ByteSize::b(o.size())))
        })
        .await
    }
}
