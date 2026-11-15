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
use worker::{Bucket, MultipartUpload, Range, UploadedPart, send::SendFuture};

/// R2 requires every part of a multipart upload, except the last, to have
/// the same size and be at least 5 MiB.
const PART_SIZE: usize = 8 * 1024 * 1024;
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
    write_policy: R2WritePolicy,
}

/// How this backend treats writes while R2 create-only publication is unproven.
///
/// Immutability needs an atomic create-only write. The Workers binding's
/// conditional put has not been proven create-only on real R2 (RFC 0030
/// section 4.2), and a multipart upload completes without any condition, so
/// neither write path can enforce it today. Writes therefore fail closed unless
/// the operator explicitly accepts a weaker guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum R2WritePolicy {
    /// Reject every write. Reads and deletes still work.
    #[default]
    FailClosed,
    /// Check for an existing object, then write. Two concurrent first writers can
    /// both pass the check, and the later write replaces the earlier one.
    NonAtomicCheckFirst,
}

impl R2Backend {
    pub fn new(bucket: Bucket, write_policy: R2WritePolicy) -> Self {
        Self {
            bucket: SendWrapper::new(bucket),
            write_policy,
        }
    }

    fn require_writable(&self) -> Result<(), Error> {
        match self.write_policy {
            R2WritePolicy::NonAtomicCheckFirst => Ok(()),
            R2WritePolicy::FailClosed => Err(Error::custom(eyre::eyre!(
                "R2 writes are disabled: create-only publication is unverified on R2. \
                 Set R2_ALLOW_NON_ATOMIC_WRITES=true to accept a non-atomic existence check"
            ))),
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

/// Best-effort create-only write. The Workers binding's conditional put is not
/// proven to be create-only on real R2 (RFC 0030 section 4.2, open question), so
/// this checks for an existing object first. That check is NOT atomic: two
/// concurrent first writers can both pass it. Replace it with a conditional put
/// once the real-R2 spike proves `If-None-Match: *` semantics, before this
/// backend is relied on for immutability.
async fn exists(bucket: &Bucket, key: &str) -> Result<bool, Error> {
    Ok(bucket.head(key).await.map_err(err)?.is_some())
}

async fn upload_multipart(
    bucket: &Bucket,
    key: &str,
    mut value: WriteStream,
) -> Result<SaveOutcome, Error> {
    if exists(bucket, key).await? {
        return Ok(SaveOutcome::AlreadyExists);
    }
    let mut buf = BytesMut::new();
    let mut upload: Option<MultipartUpload> = None;
    let mut parts: Vec<UploadedPart> = Vec::new();

    let result: Result<(), Error> = async {
        while let Some(chunk) = value.next().await {
            buf.extend_from_slice(&chunk?);

            while buf.len() >= PART_SIZE {
                let part = buf.split_to(PART_SIZE);
                if upload.is_none() {
                    upload = Some(
                        bucket
                            .create_multipart_upload(key)
                            .execute()
                            .await
                            .map_err(err)?,
                    );
                }
                let number = u16::try_from(parts.len() + 1)
                    .map_err(|e| Error::custom(eyre::eyre!(e)))?;
                let uploaded = upload
                    .as_ref()
                    .expect("upload is created above")
                    .upload_part(number, part.to_vec())
                    .await
                    .map_err(err)?;
                parts.push(uploaded);
            }
        }

        match upload.as_ref() {
            None => {
                bucket.put(key, buf.to_vec()).execute().await.map_err(err)?;
            }
            Some(upload) => {
                if !buf.is_empty() {
                    let number = u16::try_from(parts.len() + 1)
                        .map_err(|e| Error::custom(eyre::eyre!(e)))?;
                    parts.push(
                        upload
                            .upload_part(number, buf.to_vec())
                            .await
                            .map_err(err)?,
                    );
                }
            }
        }

        Ok(())
    }
    .await;

    match (result, upload) {
        (Ok(()), Some(upload)) => {
            upload.complete(parts).await.map_err(err)?;
            Ok(SaveOutcome::Stored)
        }
        (Ok(()), None) => Ok(SaveOutcome::Stored),
        (Err(e), Some(upload)) => {
            let _ = upload.abort().await;
            Err(e)
        }
        (Err(e), None) => Err(e),
    }
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
        self.require_writable()?;
        let key = self.object_key(container, key);
        SendFuture::new(async {
            if exists(&self.bucket, &key).await? {
                return Ok(SaveOutcome::AlreadyExists);
            }
            self.bucket
                .put(key, value.to_vec())
                .execute()
                .await
                .map_err(err)?;

            Ok(SaveOutcome::Stored)
        })
        .await
    }

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        self.require_writable()?;
        let key = self.object_key(container, key);
        SendFuture::new(upload_multipart(&self.bucket, &key, value)).await
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
