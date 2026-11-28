use crate::{
    SetupSys,
    secret_key::{default_store, get_secret_key},
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

        let decrypted = decrypt_config(user, remote_config_path, &file[..])?;

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
        let decrypted = decrypt_config(user, remote_config_path, &file[..])?;

        rmp_serde::from_read(&decrypted[..])
            .map_err(|e| unreadable(remote_config_path, e))?
    } else {
        omni_file_data_serde::read(remote_config_path, sys)?
    };

    Ok(remote_config)
}

/// Both readers share this, so they cannot disagree on how the key derives.
fn decrypt_config(
    user: &str,
    path: &Path,
    file: &[u8],
) -> Result<Vec<u8>, GetRemoteCachingConfigError> {
    let (service, user) = get_service_and_user(None, Some(user))?;
    let key = get_secret_key(&service, &user, default_store()?)?;
    let salt = env!("OMNI_SECRET_SALT", "remote-cache")?;
    let derived_key = derive_key_from_seed(&key, salt.as_bytes());
    crypto::decrypt(file, &derived_key[..]).map_err(|e| unreadable(path, e))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        fallback_store::Store,
        setup_remote_caching_config::write_encrypted_config,
    };
    use system_traits::impls::RealSys;

    fn config() -> RemoteCacheConfiguration {
        RemoteCacheConfiguration {
            api_key: "test-key".into(),
            api_base_url: "https://cache.invalid".into(),
            tenant_code: "tenant".into(),
            organization_code: "org".into(),
            workspace_code: "ws".into(),
            environment_code: None,
            push: false,
            target: Default::default(),
            scope: Default::default(),
            max_artifact_size: None,
            signing_key: None,
            verify: None,
            trusted_keys: None,
        }
    }

    // The only test in this crate that installs a process-wide default store.
    #[tokio::test]
    async fn the_async_and_sync_readers_decrypt_what_setup_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            Store::open_for_remote_cache(&dir.path().join("keystore")).unwrap();
        keyring_core::set_default_store(store.clone());
        let path = dir.path().join("remote-cache.omni.enc");
        let written = config();

        write_encrypted_config("reader-user", &path, &written, store, &RealSys)
            .await
            .unwrap();

        let sync =
            get_remote_caching_config("reader-user", &path, true, &RealSys)
                .unwrap();
        let asynchronous = get_remote_caching_config_async(
            "reader-user",
            &path,
            true,
            &RealSys,
        )
        .await
        .unwrap();
        keyring_core::unset_default_store();
        assert_eq!(sync.api_key, written.api_key);
        assert_eq!(asynchronous.api_key, written.api_key);
        assert_eq!(asynchronous.api_base_url, written.api_base_url);
    }
}
