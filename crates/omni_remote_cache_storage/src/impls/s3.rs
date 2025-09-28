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

use crate::{ListItem, RemoteCacheStorageBackend, error::Error};

// --- S3 Backend Struct ---
#[derive(Debug)]
pub struct S3CacheBackend {
    client: Client,
    default_container: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, new, Default)]
pub struct BasicS3Config {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint: String,
    /// This translates to the bucket name
    pub default_container: String,
    pub region: String,
    pub force_path_style: bool,
}

impl S3CacheBackend {
    pub fn from_aws_sdk_config(
        config: &SdkConfig,
        default_bucket: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::new(config),
            default_container: default_bucket.into(),
        }
    }

    pub fn from_aws_s3_sdk_config(
        config: aws_sdk_s3::Config,
        default_bucket: impl Into<String>,
    ) -> Self {
        Self {
            client: Client::from_conf(config),
            default_container: default_bucket.into(),
        }
    }

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

        Self::from_aws_s3_sdk_config(s3_config, basic.default_container.clone())
    }

    pub async fn from_aws_config_from_env(
        default_bucket: impl Into<String>,
    ) -> Self {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;

        Self::from_aws_sdk_config(&config, default_bucket)
    }
}

impl S3CacheBackend {
    fn get_bucket_name<'s>(&'s self, container: Option<&'s str>) -> &'s str {
        container.unwrap_or(&self.default_container)
    }

    async fn head(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<HeadObjectOutput, aws_sdk_s3::Error> {
        let bucket = self.get_bucket_name(container);

        let head = self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await?;

        Ok(head)
    }
}

#[async_trait]
impl RemoteCacheStorageBackend for S3CacheBackend {
    fn default_container(&self) -> &str {
        &self.default_container
    }

    async fn get(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<Option<Bytes>, Error> {
        let bucket = self.get_bucket_name(container);

        let request = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
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

    async fn list(
        &self,
        container: Option<&str>,
    ) -> Result<Vec<ListItem>, Error> {
        let bucket = self.get_bucket_name(container);

        let list = self
            .client
            .list_objects()
            .bucket(bucket)
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
        container: Option<&str>,
        key: &str,
        value: Bytes,
    ) -> Result<(), Error> {
        let bucket = self.get_bucket_name(container);
        let body = ByteStream::from(value);

        self.client
            .put_object()
            .bucket(bucket)
            .key(key)
            .body(body)
            .send()
            .await
            .map_err(Error::custom)?;

        Ok(())
    }

    async fn delete(
        &self,
        container: Option<&str>,
        key: &str,
    ) -> Result<(), Error> {
        let bucket = self.get_bucket_name(container);

        self.client
            .delete_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .map_err(Error::custom)?;

        Ok(())
    }

    async fn size(
        &self,
        container: Option<&str>,
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
