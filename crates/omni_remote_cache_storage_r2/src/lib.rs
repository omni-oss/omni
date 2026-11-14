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
    BoxStream, Container, ListItem, PageOptions, ReadStream,
    RemoteCacheStorageBackend, error::Error,
};
use send_wrapper::SendWrapper;
use worker::{Bucket, MultipartUpload, UploadedPart, send::SendFuture};

/// R2 requires every part of a multipart upload, except the last, to have
/// the same size and be at least 5 MiB.
const PART_SIZE: usize = 8 * 1024 * 1024;
const DEFAULT_PAGE_SIZE: u32 = 100;
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

async fn upload_multipart(
    bucket: &Bucket,
    key: &str,
    mut value: BoxStream<Bytes>,
) -> Result<(), Error> {
    let mut buf = BytesMut::new();
    let mut upload: Option<MultipartUpload> = None;
    let mut parts: Vec<UploadedPart> = Vec::new();

    let result: Result<(), Error> = async {
        while let Some(chunk) = value.next().await {
            buf.extend_from_slice(&chunk);

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
            Ok(())
        }
        (Ok(()), None) => Ok(()),
        (Err(e), Some(upload)) => {
            let _ = upload.abort().await;
            Err(e)
        }
        (Err(e), None) => Err(e),
    }
}

async fn list_page(
    bucket: &Bucket,
    prefix: &str,
    start_after: Option<String>,
    limit: u32,
    cursor: Option<String>,
) -> Result<(Vec<ListItem>, Option<String>), Error> {
    let mut builder = bucket.list().prefix(prefix).limit(limit);
    if let Some(start_after) = start_after {
        builder = builder.start_after(start_after);
    }
    if let Some(cursor) = cursor {
        builder = builder.cursor(cursor);
    }

    let objects = builder.execute().await.map_err(err)?;
    let items = objects
        .objects()
        .into_iter()
        .map(|o| {
            let key = o.key();
            ListItem::new(
                key.strip_prefix(prefix).unwrap_or(&key).to_string(),
                ByteSize::b(o.size()),
            )
        })
        .collect();
    let next = if objects.truncated() {
        objects.cursor()
    } else {
        None
    };

    Ok((items, next))
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

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        let prefix = self.prefix(container);
        SendFuture::new(async {
            let mut all = Vec::new();
            let mut cursor = None;
            loop {
                let (items, next) = list_page(
                    &self.bucket,
                    &prefix,
                    None,
                    MAX_LIST_LIMIT,
                    cursor,
                )
                .await?;
                all.extend(items);
                match next {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }

            Ok(all)
        })
        .await
    }

    async fn paged_list(
        &self,
        container: &Container,
        page_options: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        let prefix = self.prefix(container);
        let start_after =
            page_options.after_key.map(|k| format!("{prefix}{k}"));
        let limit = page_options
            .per_page
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .min(MAX_LIST_LIMIT);

        SendFuture::new(async {
            let (items, _) =
                list_page(&self.bucket, &prefix, start_after, limit, None)
                    .await?;

            Ok(items)
        })
        .await
    }

    async fn save(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error> {
        let key = self.object_key(container, key);
        SendFuture::new(async {
            self.bucket
                .put(key, value.to_vec())
                .execute()
                .await
                .map_err(err)?;

            Ok(())
        })
        .await
    }

    async fn save_stream(
        &self,
        container: &Container,
        key: &str,
        value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
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
