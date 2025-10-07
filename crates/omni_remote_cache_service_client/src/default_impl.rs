use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use derive_new::new;
use http::StatusCode;
use reqwest::{Client, redirect::Policy};

use crate::{
    RemoteAccessArgs, RemoteCacheServiceClient, RemoteCacheServiceClientError,
};

#[derive(Debug, Clone, new)]
pub struct DefaultRemoteCacheServiceClient {
    client: Client,
}

impl Default for DefaultRemoteCacheServiceClient {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .redirect(Policy::default())
                .connect_timeout(Duration::from_secs(30))
                .build()
                .expect("must be able to build Client"),
        }
    }
}

fn create_url(remote: &RemoteAccessArgs, digest: &str) -> String {
    format!(
        "{base_url}/v1/artifacts/{digest}?org={org}&ws={ws}&env={env}",
        base_url = remote.endpoint_base_url,
        org = remote.org,
        ws = remote.ws,
        env = remote.env,
        digest = digest,
    )
}

#[async_trait]
impl RemoteCacheServiceClient for DefaultRemoteCacheServiceClient {
    async fn get_artifact(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
    ) -> Result<Option<Bytes>, RemoteCacheServiceClientError> {
        let url = create_url(remote, digest);

        let response = self
            .client
            .get(url)
            .header("X-API-KEY", remote.api_key)
            .header("X-OMNI-TENANT", remote.tenant)
            .send()
            .await
            .map_err(RemoteCacheServiceClientError::custom)?;

        let status = response.status();

        if status.is_success() {
            let bytes = response
                .bytes()
                .await
                .map_err(RemoteCacheServiceClientError::custom)?;

            Ok(Some(bytes))
        } else {
            match status {
                StatusCode::NOT_FOUND => Ok(None),
                _ => Err(RemoteCacheServiceClientError::custom(eyre::eyre!(
                    "get artifact failed: status code {}",
                    status
                ))),
            }
        }
    }

    async fn put_artifact(
        &self,
        remote: &RemoteAccessArgs,
        digest: &str,
        artifact: Bytes,
    ) -> Result<(), RemoteCacheServiceClientError> {
        let url = create_url(remote, digest);

        let response = self
            .client
            .put(url)
            .header("X-API-KEY", remote.api_key)
            .header("X-OMNI-TENANT", remote.tenant)
            .body(artifact)
            .send()
            .await
            .map_err(RemoteCacheServiceClientError::custom)?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(RemoteCacheServiceClientError::custom(eyre::eyre!(
                "put artifact failed: status code {}",
                response.status()
            )))
        }
    }
}
