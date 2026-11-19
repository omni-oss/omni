#![allow(clippy::redundant_field_names)]

use std::collections::BTreeSet;

use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    Client,
    error::SdkError,
    operation::head_object::HeadObjectOutput,
    primitives::ByteStream,
    types::{Delete, ObjectIdentifier},
};
use bytes::Bytes;
use bytesize::ByteSize;
use derive_new::new;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::{fs::File, io::AsyncWriteExt as _};
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;

use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, ListItem, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream, error::Error,
};

const CONFLICT_RETRIES: usize = 3;
const S3_MAX_KEYS: u32 = 1000;

// --- S3 Backend Struct ---
#[derive(Debug)]
pub struct S3CacheBackend {
    client: Client,
    default_bucket: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, new, Default)]
#[allow(clippy::too_many_arguments)]
pub struct BasicS3Config {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint: String,
    pub default_bucket: String,
    pub region: String,
    pub force_path_style: bool,
}

impl S3CacheBackend {
    #[inline(always)]
    pub fn from_aws_sdk_config(
        config: &SdkConfig,
        default_bucket: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::new(config),
            default_bucket: default_bucket.into(),
        }
    }

    #[inline(always)]
    pub fn from_aws_s3_sdk_config(
        config: aws_sdk_s3::Config,
        default_bucket: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::from_conf(config),
            default_bucket: default_bucket.into(),
        }
    }

    #[inline(always)]
    pub async fn from_basic_config(basic: &BasicS3Config) -> Self {
        let credentials = Credentials::new(
            &basic.access_key_id,
            &basic.secret_access_key,
            None,
            None,
            "Static",
        );

        let config = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(Region::new(basic.region.clone()))
            .endpoint_url(&basic.endpoint)
            .load()
            .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&config)
            .force_path_style(basic.force_path_style)
            .build();

        Self::from_aws_s3_sdk_config(s3_config, basic.default_bucket.clone())
    }

    #[inline(always)]
    pub async fn from_aws_config_from_env(
        default_bucket: impl Into<String>,
    ) -> Self {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;

        Self::from_aws_sdk_config(&config, default_bucket)
    }
}

impl S3CacheBackend {
    fn key(&self, container: &Container, key: &str) -> String {
        format!("{}/{}", container.as_str(), key)
    }

    /// The key of `item` relative to `prefix`, and its size. The relative key
    /// is empty for the folder marker at the prefix itself.
    fn relative_object(
        prefix: &ContainerPrefix,
        item: aws_sdk_s3::types::Object,
    ) -> Result<(String, ByteSize), Error> {
        let key = item
            .key
            .as_deref()
            .and_then(|key| key.strip_prefix(prefix.as_str()))
            .ok_or_else(|| {
                Error::custom(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "S3 listing returned a missing or out-of-container key",
                ))
            })?;
        Ok((
            key.to_string(),
            ByteSize::b(item.size.unwrap_or_default() as u64),
        ))
    }

    fn missing_listing_field(field: &str) -> Error {
        Error::custom(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("S3 listing response is missing {field}"),
        ))
    }

    async fn exists_as_empty(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        if self.exists(container, key).await? {
            Ok(Some(Bytes::new()))
        } else {
            Ok(None)
        }
    }

    /// Deletes one batch of keys in a single `DeleteObjects` request and
    /// returns how many were accepted. Any per-key error fails the batch.
    async fn delete_keys(&self, keys: &[String]) -> Result<u64, Error> {
        let objects = keys
            .iter()
            .map(|key| {
                ObjectIdentifier::builder()
                    .key(key.as_str())
                    .build()
                    .map_err(Error::custom)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let delete = Delete::builder()
            .set_objects(Some(objects))
            .quiet(true)
            .build()
            .map_err(Error::custom)?;

        let output = self
            .client
            .delete_objects()
            .bucket(self.default_bucket.as_str())
            .delete(delete)
            .send()
            .await
            .map_err(Error::custom)?;

        if let Some(failed) = output.errors().first() {
            return Err(Error::custom(std::io::Error::other(format!(
                "S3 failed to delete {}: {} {}",
                failed.key().unwrap_or_default(),
                failed.code().unwrap_or_default(),
                failed.message().unwrap_or_default(),
            ))));
        }
        Ok(keys.len() as u64)
    }

    /// Create-only `PutObject` with `If-None-Match: *`. S3 answers `412` when
    /// the key exists (reported as `AlreadyExists`) and `409
    /// ConditionalRequestConflict` when it races a delete, which is retried
    /// a bounded number of times. The body is rebuilt for every attempt.
    async fn put_if_absent<F, Fut>(
        &self,
        container: &Container,
        key: &str,
        make_body: F,
    ) -> Result<SaveOutcome, Error>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<ByteStream, Error>>,
    {
        let object_key = self.key(container, key);
        for attempt in 0..=CONFLICT_RETRIES {
            let sent = self
                .client
                .put_object()
                .bucket(self.default_bucket.as_str())
                .key(&object_key)
                .if_none_match("*")
                .body(make_body().await?)
                .send()
                .await;
            match sent {
                Ok(_) => return Ok(SaveOutcome::Stored),
                Err(e) => match e.raw_response().map(|r| r.status().as_u16()) {
                    Some(412) => return Ok(SaveOutcome::AlreadyExists),
                    Some(409) if attempt < CONFLICT_RETRIES => continue,
                    _ => return Err(Error::custom(e)),
                },
            }
        }
        unreachable!("the final attempt always returns")
    }

    #[allow(clippy::result_large_err)]
    async fn head(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<HeadObjectOutput, aws_sdk_s3::Error> {
        let bucket = self.default_bucket.as_str();

        let head = self
            .client
            .head_object()
            .bucket(bucket)
            .key(self.key(container, key))
            .send()
            .await?;

        Ok(head)
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for S3CacheBackend {
    async fn get(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        let bucket = self.default_bucket.as_str();

        let request = self
            .client
            .get_object()
            .bucket(bucket)
            .key(self.key(container, key))
            .send()
            .await;

        match request {
            Ok(output) => {
                let data =
                    output.body.collect().await.map_err(Error::custom)?;
                Ok(Some(data.into_bytes()))
            }
            // Check for the specific error that means "Not Found" (HTTP 404)
            Err(SdkError::ServiceError(err))
                if StatusCode::from_u16(err.raw().status().as_u16())
                    .expect("should a valid status code")
                    == StatusCode::NOT_FOUND =>
            {
                Ok(None)
            }
            Err(e) => Err(Error::custom(e)),
        }
    }

    async fn exists(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<bool, Error> {
        let bucket = self.default_bucket.as_str();

        let head = self
            .client
            .head_object()
            .bucket(bucket)
            .key(self.key(container, key))
            .send()
            .await;

        match head {
            Ok(_) => Ok(true),
            // Check for the specific error that means "Not Found" (HTTP 404)
            Err(SdkError::ServiceError(err))
                if StatusCode::from_u16(err.raw().status().as_u16())
                    .expect("should a valid status code")
                    == StatusCode::NOT_FOUND =>
            {
                Ok(false)
            }
            Err(e) => Err(Error::custom(e)),
        }
    }

    async fn get_stream(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ReadStream>, Error> {
        let bucket = self.default_bucket.as_str();

        let request = self
            .client
            .get_object()
            .bucket(bucket)
            .key(self.key(container, key))
            .send()
            .await;

        match request {
            Ok(output) => {
                let data = output.body.into_async_read();

                let reader_stream =
                    ReaderStream::new(data).map(|r| r.map_err(Error::custom));

                Ok(Some(Box::pin(reader_stream)))
            }
            // Check for the specific error that means "Not Found" (HTTP 404)
            Err(SdkError::ServiceError(err))
                if StatusCode::from_u16(err.raw().status().as_u16())
                    .expect("should a valid status code")
                    == StatusCode::NOT_FOUND =>
            {
                Ok(None)
            }
            Err(e) => Err(Error::custom(e)),
        }
    }

    async fn get_range(
        &self,
        container: &Container,
        key: &str,
        range: ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        let header = match range {
            ByteRange::Suffix(0) => None,
            ByteRange::Suffix(n) => Some(format!("bytes=-{n}")),
            ByteRange::Bounded(start, end) if end <= start => None,
            ByteRange::Bounded(start, end) => {
                Some(format!("bytes={start}-{}", end - 1))
            }
        };
        // S3 rejects ranges that select nothing, so only existence is asked.
        let Some(header) = header else {
            return self.exists_as_empty(container, key).await;
        };

        let request = self
            .client
            .get_object()
            .bucket(self.default_bucket.as_str())
            .key(self.key(container, key))
            .range(header)
            .send()
            .await;

        match request {
            Ok(output) => {
                let data =
                    output.body.collect().await.map_err(Error::custom)?;
                Ok(Some(data.into_bytes()))
            }
            Err(e) => match e.raw_response().map(|r| r.status().as_u16()) {
                Some(404) => Ok(None),
                // The range starts past the end of the object. The object
                // may still exist, with nothing in the requested range.
                Some(416) => self.exists_as_empty(container, key).await,
                _ => Err(Error::custom(e)),
            },
        }
    }

    async fn list_prefix(
        &self,
        prefix: &ContainerPrefix,
        after: Option<&str>,
        limit: u32,
    ) -> Result<PrefixPage, Error> {
        let mut request = self
            .client
            .list_objects_v2()
            .bucket(self.default_bucket.as_str())
            .prefix(prefix.as_str())
            .max_keys(limit.clamp(1, S3_MAX_KEYS) as i32);
        if let Some(after) = after {
            request =
                request.start_after(format!("{}{after}", prefix.as_str()));
        }

        let list = request.send().await.map_err(Error::custom)?;
        let truncated = list.is_truncated().unwrap_or(false);

        let mut items = Vec::new();
        let mut last = None;
        for object in list.contents.unwrap_or_default() {
            let (key, size) = Self::relative_object(prefix, object)?;
            // A skipped folder marker still advances the cursor below.
            last = Some(key.clone());
            if !key.is_empty() {
                items.push(ListItem { key, size });
            }
        }

        let next_after = match (truncated, last) {
            (false, _) => None,
            (true, Some(last)) => Some(last),
            (true, None) => {
                return Err(Self::missing_listing_field(
                    "keys for a truncated page",
                ));
            }
        };
        Ok(PrefixPage { items, next_after })
    }

    async fn list_children(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        let mut names = BTreeSet::new();
        let mut token: Option<String> = None;
        loop {
            let list = self
                .client
                .list_objects_v2()
                .bucket(self.default_bucket.as_str())
                .prefix(prefix.as_str())
                .delimiter("/")
                .set_continuation_token(token.take())
                .send()
                .await
                .map_err(Error::custom)?;

            for common in list.common_prefixes() {
                let full = common
                    .prefix()
                    .ok_or_else(|| Self::missing_listing_field("a prefix"))?;
                let name = full
                    .strip_prefix(prefix.as_str())
                    .and_then(|rest| rest.strip_suffix('/'))
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        Error::custom(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "S3 listing returned an out-of-prefix child",
                        ))
                    })?;
                names.insert(name.to_string());
            }

            if !list.is_truncated().unwrap_or(false) {
                return Ok(names.into_iter().collect());
            }
            token = Some(
                list.next_continuation_token()
                    .ok_or_else(|| {
                        Self::missing_listing_field("a continuation token")
                    })?
                    .to_string(),
            );
        }
    }

    async fn delete_prefix(
        &self,
        prefix: &ContainerPrefix,
    ) -> Result<u64, Error> {
        let mut removed = 0;
        let mut token: Option<String> = None;
        loop {
            let list = self
                .client
                .list_objects_v2()
                .bucket(self.default_bucket.as_str())
                .prefix(prefix.as_str())
                .set_continuation_token(token.take())
                .send()
                .await
                .map_err(Error::custom)?;

            let truncated = list.is_truncated().unwrap_or(false);
            let next = list.next_continuation_token().map(str::to_string);

            let mut keys = Vec::new();
            for object in list.contents.unwrap_or_default() {
                let (key, _) = Self::relative_object(prefix, object)?;
                keys.push(format!("{}{key}", prefix.as_str()));
            }
            for batch in keys.chunks(S3_MAX_KEYS as usize) {
                removed += self.delete_keys(batch).await?;
            }

            if !truncated {
                return Ok(removed);
            }
            token = Some(next.ok_or_else(|| {
                Self::missing_listing_field("a continuation token")
            })?);
        }
    }

    async fn save_if_absent(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<SaveOutcome, Error> {
        self.put_if_absent(container, key, || {
            let value = value.clone();
            async move { Ok(ByteStream::from(value)) }
        })
        .await
    }

    async fn save_stream_if_absent(
        &self,
        container: &Container,
        key: &str,
        mut value: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        let tmp = tempfile::NamedTempFile::new().map_err(Error::custom)?;

        let (file, path) = tmp.into_parts();

        let mut writer = tokio::io::BufWriter::new(File::from_std(file));

        while let Some(bytes) = value.next().await {
            writer.write_all(&bytes?).await.map_err(Error::custom)?;
        }
        writer.flush().await.map_err(Error::custom)?;
        drop(writer);

        // Keep TempPath alive until every attempt has finished with the file.
        self.put_if_absent(container, key, || async {
            ByteStream::from_path(&path).await.map_err(Error::custom)
        })
        .await
    }

    async fn delete(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<(), Error> {
        let bucket = self.default_bucket.as_str();

        self.client
            .delete_object()
            .bucket(bucket)
            .key(self.key(container, key))
            .send()
            .await
            .map_err(Error::custom)?;

        Ok(())
    }

    async fn size(
        &self,
        container: &Container,
        key: &str,
    ) -> Result<Option<ByteSize>, Error> {
        let head = self.head(container, key).await;

        match head {
            Ok(output) => {
                Ok(Some(ByteSize::b(output.content_length.unwrap_or(0) as u64)))
            }
            Err(aws_sdk_s3::Error::NotFound(_)) => Ok(None),
            Err(e) => Err(Error::custom(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::types::Object;

    #[test]
    fn listing_strips_only_the_first_exact_prefix() {
        let prefix = Container::parse("ns/ci").unwrap().prefix();
        let item = Object::builder().key("ns/ci/ns/ci/d").size(7).build();
        assert_eq!(
            S3CacheBackend::relative_object(&prefix, item).unwrap().0,
            "ns/ci/d"
        );
    }

    #[test]
    fn listing_rejects_missing_and_out_of_container_keys() {
        let prefix = Container::parse("ns/ci").unwrap().prefix();
        for key in ["ns/ci2/d", "d", "ns/ci"] {
            let item = Object::builder().key(key).build();
            assert!(S3CacheBackend::relative_object(&prefix, item).is_err());
        }
        assert!(
            S3CacheBackend::relative_object(&prefix, Object::builder().build())
                .is_err()
        );
    }

    #[test]
    fn listing_yields_an_empty_key_for_the_exact_container_marker() {
        let prefix = Container::parse("ns/ci").unwrap().prefix();
        let marker = Object::builder().key("ns/ci/").size(0).build();
        assert_eq!(
            S3CacheBackend::relative_object(&prefix, marker).unwrap().0,
            ""
        );
    }
}
