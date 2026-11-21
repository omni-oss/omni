use bon::Builder;
use directories::ProjectDirs;
use tracing::Level;

use crate::{
    fallback_store,
    secret_key::{SecretKeyError, SecretKeyErrorInner},
};

#[derive(Builder, Default, Debug)]
pub struct InitConfig {
    #[builder(default)]
    pub use_fallback_store: bool,
}

#[tracing::instrument(level = Level::DEBUG)]
pub fn initialize(config: InitConfig) -> eyre::Result<()> {
    tracing::trace!("initializing_omni_setup");

    if config.use_fallback_store {
        return Ok(init_fallback()?);
    }

    let result: keyring_core::Result<()> = try {
        cfg_select! {
            target_os = "macos" => {
                let store = apple_native_keyring_store::keychain::Store::new()?;
                keyring_core::set_default_store(store);
            }
            target_os = "windows" => {
                let store = windows_native_keyring_store::store::Store::new()?;
                keyring_core::set_default_store(store);
            }
            target_os = "linux" => {
                let result = zbus_secret_service_keyring_store::Store::new();

                match result {
                    Ok(store) => {
                        keyring_core::set_default_store(store);
                    }
                    Err(error) => match error {
                        keyring_core::Error::PlatformFailure(error) => {
                            // normalize the text
                            let text_err = error.to_string().to_lowercase();

                            if text_err.contains("the name org.freedesktop.secrets was not provided by any .service files") {
                                let store = linux_keyutils_keyring_store::Store::new()?;
                                keyring_core::set_default_store(store);
                            } else {
                                Err(keyring_core::Error::PlatformFailure(error))?;
                            }
                        }
                        error => {
                            Err(error)?;
                        }
                    },
                }
            }
            _ => {
                Err(keyring_core::Error::NoDefaultStore)?;
            }
        }
    };

    Ok(initialize_store(result, init_fallback)?)
}

fn initialize_store(
    native: keyring_core::Result<()>,
    fallback: impl FnOnce() -> Result<(), SecretKeyError>,
) -> Result<(), SecretKeyError> {
    if native.is_err() {
        // Platform errors can carry backend data. Log the recovery only.
        tracing::warn!(
            "Native credential store unavailable; trying the fallback store"
        );
        fallback()?;
    }

    tracing::trace!("initialized_omni_setup");
    Ok(())
}

fn init_fallback() -> Result<(), SecretKeyError> {
    let project_dirs = ProjectDirs::from("com", "omni-oss", "omni")
        .ok_or(SecretKeyErrorInner::Keyring)?;
    let backing_file = project_dirs.config_dir().join("keystore");
    let keystore = fallback_store::Store::open_for_remote_cache(&backing_file)?;
    keyring_core::set_default_store(keystore);
    Ok(())
}

pub fn deinitialize() -> eyre::Result<()> {
    tracing::trace!("deinitializing_omni_setup");
    keyring_core::unset_default_store();
    tracing::trace!("deinitialized_omni_setup");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_native_store_tries_fallback_for_non_platform_errors() {
        let mut used_fallback = false;
        initialize_store(Err(keyring_core::Error::NoDefaultStore), || {
            used_fallback = true;
            Ok(())
        })
        .unwrap();
        assert!(used_fallback);
    }

    #[test]
    fn failed_fallback_reports_an_environment_error_without_backend_text() {
        let native_secret = "native-backend-sentinel";
        let fallback_secret = "fallback-backend-sentinel";
        let error = initialize_store(
            Err(keyring_core::Error::PlatformFailure(Box::new(
                std::io::Error::other(native_secret),
            ))),
            || {
                Err(SecretKeyError::from(keyring_core::Error::NoStorageAccess(
                    Box::new(std::io::Error::other(fallback_secret)),
                )))
            },
        )
        .unwrap_err();
        let report = eyre::Report::new(error);
        let diagnostic =
            omni_diagnostic::DiagnosticReport::from_report(&report).unwrap();
        assert_eq!(
            diagnostic.to_json()["kind"],
            "remote_cache.keyring_unavailable"
        );
        assert_eq!(diagnostic.exit_code(), 1);
        for output in [
            format!("{report:?}"),
            diagnostic.to_text(),
            diagnostic.to_json().to_string(),
        ] {
            assert!(!output.contains(native_secret));
            assert!(!output.contains(fallback_secret));
        }
    }
}
