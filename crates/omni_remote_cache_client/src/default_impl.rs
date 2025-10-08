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

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use crate::{
        DefaultRemoteCacheServiceClient, RemoteAccessArgs,
        RemoteCacheServiceClient, test_utils::ChildProcessGuard,
    };

    const DEFAULT_DIGEST: &str =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const DEFAULT_TENANT: &str = "tenant1";
    const DEFAULT_ORG: &str = "org1";
    const DEFAULT_WS: &str = "ws1";
    const DEFAULT_ENV: &str = "env1";
    const DEFAULT_API_KEY: &str = "key1";
    const DEFAULT_BODY: Bytes = Bytes::from_static(b"hello world");

    fn def_remote_access_args<'a>(base_url: &'a str) -> RemoteAccessArgs<'a> {
        RemoteAccessArgs {
            api_key: DEFAULT_API_KEY,
            endpoint_base_url: base_url,
            env: DEFAULT_ENV,
            org: DEFAULT_ORG,
            tenant: DEFAULT_TENANT,
            ws: DEFAULT_WS,
        }
    }

    #[tokio::test]
    async fn test_put_artifact() {
        let guard = ChildProcessGuard::new();
        let client = DefaultRemoteCacheServiceClient::default();
        let remote = def_remote_access_args(&guard.api_base_url);

        let resp = client
            .put_artifact(&remote, DEFAULT_DIGEST, DEFAULT_BODY)
            .await;

        assert!(resp.is_ok(), "put_artifact failed: {:?}", resp);
    }

    #[tokio::test]
    async fn test_get_artifact() {
        let guard = ChildProcessGuard::new();
        let client = DefaultRemoteCacheServiceClient::default();
        let remote = def_remote_access_args(&guard.api_base_url);

        let resp = client
            .put_artifact(&remote, DEFAULT_DIGEST, DEFAULT_BODY)
            .await;

        assert!(resp.is_ok(), "put_artifact failed: {:?}", resp);

        let resp = client.get_artifact(&remote, DEFAULT_DIGEST).await;

        assert!(resp.is_ok(), "get_artifact failed: {:?}", resp);
        assert_eq!(resp.unwrap().unwrap(), DEFAULT_BODY);
    }

    #[tokio::test]
    async fn test_get_artifact_not_found() {
        let guard = ChildProcessGuard::new();
        let client = DefaultRemoteCacheServiceClient::default();
        let remote = def_remote_access_args(&guard.api_base_url);

        let resp = client.get_artifact(&remote, DEFAULT_DIGEST).await;

        assert!(resp.is_ok(), "get_artifact failed: {:?}", resp);
        assert!(resp.unwrap().is_none());
    }
}
