use crate::{
    SetupSys,
    util::{env, get_service_and_user},
};
use std::path::Path;

use derive_new::new;
use omni_configurations::RemoteCacheConfiguration;
use omni_remote_cache_client::{
    RemoteAccessArgs, RemoteCacheClient, RemoteCacheClientError,
};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

use crate::{
    crypto, derive_key::derive_key_from_seed, secret_key::get_secret_key,
};

#[allow(clippy::too_many_arguments)]
pub async fn setup_remote_caching_config_async<TClient: RemoteCacheClient>(
    user: &str,
    client: &TClient,
    remote_config_path: &Path,
    api_base_url: &str,
    api_key: &str,
    tenant_code: &str,
    organization_code: &str,
    workspace_code: &str,
    environment_code: Option<&str>,
    encrypt: bool,
    sys: &impl SetupSys,
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

    sys.fs_create_dir_all_async(parent).await?;

    if encrypt {
        let (service, user) = get_service_and_user(None, Some(user))?;
        let secret_key = get_secret_key(
            &service,
            &user,
            keyring_core::get_default_store()
                .expect("default store is not set"),
        )?;
        let salt = env!("OMNI_SECRET_SALT", "remote-cache")?;
        let derived_key = derive_key_from_seed(&secret_key, salt.as_bytes());

        let encrypted = crypto::encrypt(
            rmp_serde::to_vec(&remote_config)?.as_slice(),
            &derived_key[..],
        )?;
        sys.fs_write_async(remote_config_path, &encrypted).await?;
    } else {
        omni_file_data_serde::write_async(
            remote_config_path,
            &remote_config,
            sys,
        )
        .await?;
    }

    Ok(())
}

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error("failed to setup remote caching: {0}")]
#[diagnostic(forward)]
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

#[derive(
    Debug,
    thiserror::Error,
    EnumDiscriminants,
    EnumIs,
    new,
    omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(vis(pub), name(SetupRemoteCachingConfigErrorKind))]
pub(crate) enum SetupRemoteCachingConfigErrorInner {
    #[error(transparent)]
    #[diagnostic(skip)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    RemoteCacheClient(#[from] RemoteCacheClientError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Encode(#[from] omni_file_data_serde::Error),

    #[error(transparent)]
    #[diagnostic(skip)]
    RmpSerdeEncode(#[from] rmp_serde::encode::Error),

    #[error(transparent)]
    #[diagnostic(delegate)]
    SecretKey(#[from] crate::secret_key::SecretKeyError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Custom(#[from] eyre::Report),

    /// The remote cache server rejected the access details.
    #[error("invalid access")]
    #[diagnostic(
        code = "remote_cache.invalid_access",
        hint = "check the API key, tenant, organization, workspace and environment codes, then run remote caching setup again"
    )]
    InvalidAccess(#[source] eyre::Report),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Crypto(#[from] crypto::CryptoError),

    #[error(transparent)]
    #[diagnostic(skip)]
    Env(#[from] std::env::VarError),
}
