use crate::util::env;
use std::path::Path;

use derive_new::new;
use omni_configurations::RemoteCacheConfiguration;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};
use system_traits::impls::RealSys;

use crate::{
    crypto::{self, CryptoError},
    derive_key::derive_key_from_seed,
    secret_key::get_secret_key,
};

pub async fn get_remote_caching_config_async(
    remote_config_path: &Path,
    decrypt: bool,
) -> Result<RemoteCacheConfiguration, GetRemoteCachingConfigError> {
    let remote_config: RemoteCacheConfiguration = if decrypt {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(remote_config_path)?;
        let key = crate::secret_key::get_secret_key()?;
        let decrypted = crypto::decrypt(&file, key.as_bytes())?;

        rmp_serde::from_read(&decrypted[..])?
    } else {
        omni_file_data_serde::read_async(remote_config_path, &RealSys).await?
    };

    Ok(remote_config)
}

pub fn get_remote_caching_config_sync(
    remote_config_path: &Path,
    decrypt: bool,
) -> Result<RemoteCacheConfiguration, GetRemoteCachingConfigError> {
    let remote_config: RemoteCacheConfiguration = if decrypt {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(remote_config_path)?;
        let secret_key = get_secret_key()?;
        let salt = env!("OMNI_SECRET_SALT", "remote-cache")?;
        let derived_key = derive_key_from_seed(&secret_key, salt.as_bytes());
        let decrypted = crypto::decrypt(&file, &derived_key[..])?;

        rmp_serde::from_read(&decrypted[..])?
    } else {
        omni_file_data_serde::read(remote_config_path, &RealSys)?
    };

    Ok(remote_config)
}

#[derive(Debug, thiserror::Error, new)]
#[error(transparent)]
pub struct GetRemoteCachingConfigError(GetRemoteCachingConfigErrorInner);

impl GetRemoteCachingConfigError {
    #[allow(unused)]
    pub fn kind(&self) -> GetRemoteCachingConfigErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<GetRemoteCachingConfigErrorInner>> From<T>
    for GetRemoteCachingConfigError
{
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs, new)]
#[strum_discriminants(vis(pub), name(GetRemoteCachingConfigErrorKind))]
enum GetRemoteCachingConfigErrorInner {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Decode(#[from] omni_file_data_serde::Error),

    #[error(transparent)]
    RmpSerdeDecode(#[from] rmp_serde::decode::Error),

    #[error(transparent)]
    SecretKey(#[from] crate::secret_key::SecretKeyError),

    #[error(transparent)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    Crypto(#[from] CryptoError),

    #[error(transparent)]
    RemoteCache(#[from] omni_remote_cache_client::RemoteCacheClientError),

    #[error(transparent)]
    Env(#[from] std::env::VarError),
}
