use crate::{
    SetupSys,
    secret_key::get_secret_key,
    util::{env, get_service_and_user},
};
use std::path::{Path, PathBuf};

use derive_new::new;
use omni_configurations::RemoteCacheConfiguration;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

use crate::{crypto, derive_key::derive_key_from_seed};

pub async fn get_remote_caching_config_async(
    user: &str,
    remote_config_path: &Path,
    decrypt: bool,
    sys: &impl SetupSys,
) -> Result<RemoteCacheConfiguration, GetRemoteCachingConfigError> {
    let remote_config: RemoteCacheConfiguration = if decrypt {
        let file = sys
            .fs_read_async(remote_config_path)
            .await
            .map_err(|e| unreadable(remote_config_path, e))?;

        let (service, user) = get_service_and_user(None, Some(user))?;
        let key = get_secret_key(
            &service,
            &user,
            keyring_core::get_default_store()
                .expect("default store is not set"),
        )?;
        let decrypted = crypto::decrypt(&file[..], key.as_bytes())
            .map_err(|e| unreadable(remote_config_path, e))?;

        rmp_serde::from_read(&decrypted[..])
            .map_err(|e| unreadable(remote_config_path, e))?
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
        let file = sys
            .fs_read(remote_config_path)
            .map_err(|e| unreadable(remote_config_path, e))?;
        let (service, user) = get_service_and_user(None, Some(user))?;
        let key = get_secret_key(
            &service,
            &user,
            keyring_core::get_default_store()
                .expect("default store is not set"),
        )?;
        let salt = env!("OMNI_SECRET_SALT", "remote-cache")?;
        let derived_key = derive_key_from_seed(&key, salt.as_bytes());
        let decrypted = crypto::decrypt(&file[..], &derived_key[..])
            .map_err(|e| unreadable(remote_config_path, e))?;

        rmp_serde::from_read(&decrypted[..])
            .map_err(|e| unreadable(remote_config_path, e))?
    } else {
        omni_file_data_serde::read(remote_config_path, sys)?
    };

    Ok(remote_config)
}

fn unreadable(
    path: &Path,
    source: impl Into<eyre::Report>,
) -> GetRemoteCachingConfigError {
    GetRemoteCachingConfigErrorInner::ConfigUnreadable {
        path: path.to_path_buf(),
        source: source.into(),
    }
    .into()
}

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
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
        Self(inner)
    }
}

#[derive(
    Debug,
    thiserror::Error,
    EnumDiscriminants,
    EnumIs,
    omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(GetRemoteCachingConfigErrorKind))]
pub(crate) enum GetRemoteCachingConfigErrorInner {
    /// The stored remote caching config could not be opened or decrypted.
    #[error("failed to open remote caching config at '{path}'")]
    #[diagnostic(
        code = "remote_cache.config_unreadable",
        audience = environment,
        hint = "re-run remote caching setup to recreate the config at {path:?}"
    )]
    ConfigUnreadable {
        #[diagnostic(detail)]
        path: PathBuf,
        #[source]
        source: eyre::Report,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    Decode(#[from] omni_file_data_serde::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    SecretKey(#[from] crate::secret_key::SecretKeyError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    #[error(transparent)]
    #[diagnostic(delegate)]
    RemoteCache(#[from] omni_remote_cache_client::RemoteCacheClientError),

    #[error(transparent)]
    #[diagnostic(skip)]
    Env(#[from] std::env::VarError),
}
