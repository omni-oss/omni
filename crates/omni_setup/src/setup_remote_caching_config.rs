use crate::{
    SetupSys,
    util::{env, get_service_and_user},
};
use std::path::Path;

use derive_new::new;
use omni_configurations::{
    RemoteCacheConfiguration, RemoteCacheSettings, RemoteTarget, ScopeConfig,
};
use omni_remote_cache_client::{
    RemoteAccessArgs, RemoteCacheClient, RemoteCacheClientError,
};
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant as _};

use crate::{
    crypto, derive_key::derive_key_from_seed, secret_key::get_secret_key,
};

/// Decides whether a probe result can be saved.
///
/// A server that reports its scopes lets setup refuse a credential that could
/// neither read nor write the cache (for example one that can only list or
/// publish), instead of saving a configuration that fails on every task. A
/// server that reports none (an older one) is accepted as before.
fn check_access(
    result: &omni_remote_cache_client::ValidateAccessResult,
) -> Result<(), SetupRemoteCachingConfigErrorInner> {
    if !result.is_valid {
        return Err(SetupRemoteCachingConfigErrorInner::InvalidAccess(
            eyre::Report::msg(
                result
                    .message
                    .as_deref()
                    .unwrap_or("invalid access")
                    .to_string(),
            ),
        ));
    }
    let held = |scope: &str| result.scopes.iter().any(|s| s == scope);
    if !result.scopes.is_empty()
        && !held("read:artifacts")
        && !held("write:artifacts")
        && !held("publish:artifacts")
    {
        return Err(SetupRemoteCachingConfigErrorInner::InsufficientScopes {
            scopes: result.scopes.join(", "),
        });
    }
    Ok(())
}

/// Tells the user, at setup, about settings the server's answer says cannot
/// work. The configuration is still saved: scopes can change and the server
/// enforces them on every request.
fn warn_about_settings(
    result: &omni_remote_cache_client::ValidateAccessResult,
    settings: &RemoteCacheSettings,
) {
    let held = |scope: &str| result.scopes.iter().any(|s| s == scope);
    if !result.scopes.is_empty() {
        if settings.target == RemoteTarget::Shared && !held("publish:artifacts")
        {
            tracing::warn!(
                "`--target shared` needs `publish:artifacts`, which this credential does not hold; uploads will be refused"
            );
        }
        if settings.push
            && settings.target == RemoteTarget::Isolated
            && !held("write:artifacts")
            && !held("publish:artifacts")
        {
            tracing::warn!(
                "`--push` needs `write:artifacts` or `publish:artifacts`, which this credential does not hold; uploads will be refused"
            );
        }
    }
    if let Some(scope) = &result.scope {
        tracing::info!(%scope, "the server will use this isolated scope label");
    }
    if !settings.push {
        tracing::info!(
            "uploads are off; pass `--push` to upload results to the remote cache"
        );
    }
}

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
    settings: RemoteCacheSettings,
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
            placement: Default::default(),
            scope: match &settings.scope {
                ScopeConfig::Literal(label) => Some(label.as_str()),
                _ => None,
            },
            max_artifact_size: None,
            signature: None,
        })
        .await?;

    check_access(&result)?;
    warn_about_settings(&result, &settings);

    let remote_config = RemoteCacheConfiguration {
        api_key: api_key.to_string(),
        api_base_url: api_base_url.to_string(),
        tenant_code: tenant_code.to_string(),
        organization_code: organization_code.to_string(),
        workspace_code: workspace_code.to_string(),
        environment_code: environment_code.map(|s| s.to_string()),
        push: settings.push,
        target: settings.target,
        scope: settings.scope.clone(),
        max_artifact_size: settings.max_artifact_size,
        signing_key: settings.signing_key.clone(),
        verify: settings.verify,
        trusted_keys: settings.trusted_keys.clone(),
    };

    let parent = remote_config_path.parent().expect("should have parent");

    sys.fs_create_dir_all_async(parent)
        .await
        .map_err(|source| {
            SetupRemoteCachingConfigErrorInner::ConfigWriteFailed {
                path: remote_config_path.to_path_buf(),
                source,
            }
        })?;

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
        sys.fs_write_async(remote_config_path, &encrypted)
            .await
            .map_err(|source| {
                SetupRemoteCachingConfigErrorInner::ConfigWriteFailed {
                    path: remote_config_path.to_path_buf(),
                    source,
                }
            })?;
    } else {
        omni_file_data_serde::write_async(
            remote_config_path,
            &remote_config,
            sys,
        )
        .await
        .map_err(|source| {
            SetupRemoteCachingConfigErrorInner::ConfigEncodeFailed {
                path: remote_config_path.to_path_buf(),
                source,
            }
        })?;
    }

    Ok(())
}

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error("failed to setup remote caching")]
#[diagnostic(forward)]
pub struct SetupRemoteCachingConfigError(
    #[source] SetupRemoteCachingConfigErrorInner,
);

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
    /// The remote caching configuration could not be saved.
    #[error("failed to write remote caching configuration at '{path}'", path = path.display())]
    #[diagnostic(code = "remote_cache.config_write_failed", audience = environment, hint = "check disk space and write access to {path:?}, then retry remote caching setup")]
    ConfigWriteFailed {
        #[diagnostic(detail)]
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The plaintext remote caching configuration could not be saved.
    #[error("failed to save remote caching configuration at '{path}'", path = path.display())]
    #[diagnostic(code = "remote_cache.config_save_failed", audience = environment, hint = "check the configuration file format and write access to {path:?}, then retry remote caching setup")]
    ConfigEncodeFailed {
        #[diagnostic(detail)]
        path: std::path::PathBuf,
        #[source]
        source: omni_file_data_serde::Error,
    },

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
    ///
    /// The server's message is deliberately not a `#[source]`: it comes from an
    /// external server and could echo credentials.
    #[error("invalid access")]
    #[diagnostic(
        code = "remote_cache.invalid_access",
        hint = "check the API key, tenant, organization, workspace and environment codes, then run remote caching setup again"
    )]
    InvalidAccess(eyre::Report),

    /// The credential is valid for the target but cannot use the cache.
    #[error(
        "the credential cannot read or write artifacts (it holds: {scopes})"
    )]
    #[diagnostic(
        code = "remote_cache.insufficient_scopes",
        hint = "grant the API key read:artifacts and/or write:artifacts for this environment, then run remote caching setup again"
    )]
    InsufficientScopes {
        #[diagnostic(detail)]
        scopes: String,
    },

    #[error(transparent)]
    #[diagnostic(delegate)]
    Crypto(#[from] crypto::CryptoError),

    #[error(transparent)]
    #[diagnostic(skip)]
    Env(#[from] std::env::VarError),
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;
    use eyre::WrapErr;
    use omni_diagnostic::{Audience, find};

    #[test]
    fn config_write_failure_has_path_and_retains_os_cause() {
        let error: SetupRemoteCachingConfigError =
            SetupRemoteCachingConfigErrorInner::ConfigWriteFailed {
                path: "remote-cache.enc".into(),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "permission denied",
                ),
            }
            .into();
        let report = Err::<(), _>(error)
            .wrap_err("setting up remote caching")
            .unwrap_err();
        let diagnostic = find(&report).expect("write failure diagnostic");
        assert_eq!(diagnostic.code(), "remote_cache.config_write_failed");
        assert_eq!(diagnostic.audience(), Audience::Environment);
        assert_eq!(diagnostic.details().unwrap()["path"], "remote-cache.enc");
        // The cause chain must reach the OS error.
        let chain = report
            .chain()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(chain.contains("remote-cache.enc"), "{chain}");
        assert!(chain.contains("permission denied"), "{chain}");
        assert!(omni_codes::get("remote_cache.config_write_failed").is_some());
        assert!(omni_codes::get("remote_cache.config_save_failed").is_some());
    }
}

#[cfg(test)]
mod tests {
    use omni_remote_cache_client::ValidateAccessResult;

    use super::{SetupRemoteCachingConfigErrorInner as Inner, check_access};

    fn result(valid: bool, scopes: &[&str]) -> ValidateAccessResult {
        let mut result =
            ValidateAccessResult::new(valid, Some("denied".into()));
        result.scopes = scopes.iter().map(|s| s.to_string()).collect();
        result
    }

    #[test]
    fn an_invalid_probe_is_invalid_access() {
        assert!(matches!(
            check_access(&result(false, &[])),
            Err(Inner::InvalidAccess(_))
        ));
    }

    #[test]
    fn a_credential_that_can_read_or_write_is_accepted() {
        for scopes in [
            vec!["read:artifacts"],
            vec!["write:artifacts"],
            vec!["publish:artifacts"],
            vec!["list:artifacts", "write:artifacts", "publish:artifacts"],
        ] {
            assert!(check_access(&result(true, &scopes)).is_ok(), "{scopes:?}");
        }
    }

    #[test]
    fn a_credential_that_can_neither_read_nor_write_is_refused_with_its_scopes()
    {
        for scopes in [
            vec!["delete:artifacts"],
            vec!["list:artifacts", "delete:artifacts"],
        ] {
            match check_access(&result(true, &scopes)) {
                Err(Inner::InsufficientScopes { scopes: held }) => {
                    assert_eq!(held, scopes.join(", "));
                }
                other => panic!("{scopes:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_server_that_reports_no_scopes_is_accepted_as_before() {
        assert!(check_access(&result(true, &[])).is_ok());
    }
}
