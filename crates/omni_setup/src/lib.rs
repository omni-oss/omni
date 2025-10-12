use std::{fs::OpenOptions, path::Path};

use derive_new::new;
use omni_configurations::RemoteCacheConfiguration;
use omni_remote_cache_client::{
    RemoteAccessArgs, RemoteCacheClient, RemoteCacheClientError,
};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

pub async fn setup_remote_caching<TClient: RemoteCacheClient>(
    client: &TClient,
    remote_config_path: &Path,
    api_base_url: &str,
    api_key: &str,
    tenant_code: &str,
    organization_code: &str,
    workspace_code: &str,
    environment_code: Option<&str>,
) -> Result<(), SetupRemoteCachingError> {
    let result = client
        .validate_access(&RemoteAccessArgs {
            api_base_url,
            api_key,
            env: environment_code.unwrap_or("default"),
            org: organization_code,
            tenant: tenant_code,
            ws: workspace_code,
        })
        .await?;

    if !result.is_valid {
        return Err(SetupRemoteCachingErrorInner::InvalidAccess(
            eyre::Report::msg(
                result
                    .message
                    .as_deref()
                    .unwrap_or("invalid access")
                    .to_string(),
            ),
        )
        .into());
    }

    let remote_config = RemoteCacheConfiguration {
        api_key: api_key.to_string(),
        api_base_url: api_base_url.to_string(),
        tenant_code: tenant_code.to_string(),
        organization_code: organization_code.to_string(),
        workspace_code: workspace_code.to_string(),
        environment_code: environment_code.map(|s| s.to_string()),
    };

    let parent = remote_config_path.parent().expect("should have parent");

    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&remote_config_path)?;

    serde_norway::to_writer(&mut file, &remote_config)?;

    Ok(())
}

#[derive(Debug, thiserror::Error, new)]
#[error("failed to setup remote caching: {inner}")]
pub struct SetupRemoteCachingError {
    kind: SetupRemoteCachingErrorKind,
    inner: SetupRemoteCachingErrorInner,
}

impl SetupRemoteCachingError {
    #[allow(unused)]
    pub fn kind(&self) -> SetupRemoteCachingErrorKind {
        self.kind
    }
}

impl<T: Into<SetupRemoteCachingErrorInner>> From<T>
    for SetupRemoteCachingError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self {
            kind: inner.discriminant(),
            inner: inner.into(),
        }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs, new)]
#[strum_discriminants(vis(pub), name(SetupRemoteCachingErrorKind))]
enum SetupRemoteCachingErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    RemoteCacheClient(#[from] RemoteCacheClientError),

    #[error(transparent)]
    SerdeNorway(#[from] serde_norway::Error),

    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("invalid access")]
    InvalidAccess(#[source] eyre::Report),
}
