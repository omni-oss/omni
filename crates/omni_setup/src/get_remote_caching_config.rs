use crate::{
    SetupSys,
    secret_key::get_secret_key,
    util::{env, get_service_and_user},
};
use std::path::Path;

use derive_new::new;
use omni_configurations::RemoteCacheConfiguration;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

use crate::{
    crypto::{self, CryptoError},
    derive_key::derive_key_from_seed,
};

pub async fn get_remote_caching_config_async(
    user: &str,
    remote_config_path: &Path,
    decrypt: bool,
    sys: &impl SetupSys,
) -> Result<RemoteCacheConfiguration, GetRemoteCachingConfigError> {
    let remote_config: RemoteCacheConfiguration = if decrypt {
        let file = sys.fs_read_async(remote_config_path).await?;

        let (service, user) = get_service_and_user(None, Some(user))?;
        let key = get_secret_key(
            &service,
            &user,
            keyring_core::get_default_store()
                .expect("default store is not set"),
        )?;
        let decrypted = crypto::decrypt(&file[..], key.as_bytes())?;

        rmp_serde::from_read(&decrypted[..])?
    } else {
        omni_file_data_serde::read_async(remote_config_path, sys).await?
    };

    Ok(remote_config)
}

pub fn get_remote_caching_config(
    user: &str,
    remote_config_path: &Path,
    decrypt: bool,
    sys: &impl SetupSys,
) -> Result<RemoteCacheConfiguration, GetRemoteCachingConfigError> {
    let remote_config: RemoteCacheConfiguration = if decrypt {
        let file = sys.fs_read(remote_config_path)?;
        let (service, user) = get_service_and_user(None, Some(user))?;
        let key = get_secret_key(
            &service,
            &user,
            keyring_core::get_default_store()
                .expect("default store is not set"),
        )?;
        let salt = env!("OMNI_SECRET_SALT", "remote-cache")?;
        let derived_key = derive_key_from_seed(&key, salt.as_bytes());
        let decrypted = crypto::decrypt(&file[..], &derived_key[..])?;

        rmp_serde::from_read(&decrypted[..])?
    } else {
        omni_file_data_serde::read(remote_config_path, sys)?
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
