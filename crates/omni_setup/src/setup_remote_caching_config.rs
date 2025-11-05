use crate::util::env;
use std::{fs::OpenOptions, io::Write as _, path::Path};

use derive_new::new;
use omni_configurations::RemoteCacheConfiguration;
use omni_remote_cache_client::{
    RemoteAccessArgs, RemoteCacheClient, RemoteCacheClientError,
};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};
use system_traits::impls::RealSys;

use crate::{
    crypto, derive_key::derive_key_from_seed, secret_key::get_secret_key,
};

pub async fn setup_remote_caching_config<TClient: RemoteCacheClient>(
    client: &TClient,
    remote_config_path: &Path,
    api_base_url: &str,
    api_key: &str,
    tenant_code: &str,
    organization_code: &str,
    workspace_code: &str,
    environment_code: Option<&str>,
    encrypt: bool,
) -> Result<(), SetupRemoteCachingConfigError> {
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
        return Err(SetupRemoteCachingConfigErrorInner::InvalidAccess(
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

    if encrypt {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&remote_config_path)?;

        let secret_key = get_secret_key()?;
        let salt = env!("OMNI_SECRET_SALT", "remote-cache")?;
        let derived_key = derive_key_from_seed(&secret_key, salt.as_bytes());

        let encrypted = crypto::encrypt(
            rmp_serde::to_vec(&remote_config)?.as_slice(),
            &derived_key[..],
        )?;
        file.write_all(&encrypted)?;
    } else {
        omni_file_data_serde::write_async(
            remote_config_path,
            &remote_config,
            &RealSys,
        )
        .await?;
    }

    Ok(())
}

#[derive(Debug, thiserror::Error, new)]
#[error("failed to setup remote caching: {0}")]
pub struct SetupRemoteCachingConfigError(SetupRemoteCachingConfigErrorInner);

impl SetupRemoteCachingConfigError {
    #[allow(unused)]
    pub fn kind(&self) -> SetupRemoteCachingConfigErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<SetupRemoteCachingConfigErrorInner>> From<T>
    for SetupRemoteCachingConfigError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner)
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs, new)]
#[strum_discriminants(vis(pub), name(SetupRemoteCachingConfigErrorKind))]
enum SetupRemoteCachingConfigErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    RemoteCacheClient(#[from] RemoteCacheClientError),

    #[error(transparent)]
    Encode(#[from] omni_file_data_serde::Error),

    #[error(transparent)]
    RmpSerdeEncode(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    SecretKey(#[from] crate::secret_key::SecretKeyError),

    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error("invalid access")]
    InvalidAccess(#[source] eyre::Report),

    #[error(transparent)]
    Crypto(#[from] crypto::CryptoError),

    #[error(transparent)]
    Env(#[from] std::env::VarError),
}
