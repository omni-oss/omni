#![allow(clippy::redundant_field_names)]

use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    Client, error::SdkError, operation::head_object::HeadObjectOutput,
    primitives::ByteStream,
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
    BoxStream, Container, ListItem, PageOptions, ReadStream,
    RemoteCacheStorageBackend, error::Error,
};

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

    async fn list(
        &self,
        container: &Container,
    ) -> Result<Vec<ListItem>, Error> {
        let bucket = self.default_bucket.as_str();

        let list = self
            .client
            .list_objects()
            .bucket(bucket)
            .prefix(container.prefix().as_str())
            .send()
            .await
            .map_err(Error::custom)?;

        let mut items = Vec::new();
        for item in list.contents.unwrap_or_default() {
            items.push(ListItem {
                key: item.key.unwrap_or_default(),
                size: ByteSize::b(item.size.unwrap_or_default() as u64),
            });
        }

        Ok(items)
    }

    async fn paged_list(
        &self,
        container: &Container,
        query: PageOptions,
    ) -> Result<Vec<ListItem>, Error> {
        let bucket = self.default_bucket.as_str();

        let list = self
            .client
            .list_objects_v2()
            .max_keys(query.per_page.unwrap_or(100) as i32)
            .bucket(bucket)
            .prefix(container.prefix().as_str())
            .send()
            .await
            .map_err(Error::custom)?;

        let mut items = Vec::new();
        for item in list.contents.unwrap_or_default() {
            items.push(ListItem {
                key: item.key.unwrap_or_default(),
                size: ByteSize::b(item.size.unwrap_or_default() as u64),
            });
        }

        Ok(items)
    }

    async fn save(
        &self,
        container: &Container,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error> {
        let bucket = self.default_bucket.as_str();
        let body = ByteStream::from(value);

        self.client
            .put_object()
            .bucket(bucket)
            .key(self.key(container, key))
            .body(body)
            .send()
            .await
            .map_err(Error::custom)?;

        Ok(())
    }

    async fn save_stream(
        &self,
        container: &Container,
        key: &str,
        mut value: BoxStream<Bytes>,
    ) -> Result<(), Error> {
        let bucket = self.default_bucket.as_str();

        let tmp = tempfile::NamedTempFile::new().map_err(Error::custom)?;

        let (file, path) = tmp.into_parts();

        let mut writer = tokio::io::BufWriter::new(File::from_std(file));

        while let Some(bytes) = value.next().await {
            writer.write_all(&bytes).await.map_err(Error::custom)?;
        }

        let body = ByteStream::from_path(path).await.map_err(Error::custom)?;

        self.client
            .put_object()
            .bucket(bucket)
            .key(self.key(container, key))
            .body(body)
            .send()
            .await
            .map_err(Error::custom)?;

        Ok(())
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
            .key(key)
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
