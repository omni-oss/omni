use bon::Builder;
use directories::ProjectDirs;
use tracing::Level;

use crate::fallback_store;

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
                return erye::erye!(
                    "Unsupported platform, omni_setup only supports macOS, Windows and Linux"
                );
            }
        }
    };

    if let Err(keyring_core::Error::PlatformFailure(error)) = result {
        tracing::error!(%error, "keyring_store_platform_failure");
        tracing::warn!(
            "Using fallback store due to keyring store platform failure"
        );
        init_fallback()?;
    } else {
        tracing::trace!("initialized_omni_setup");
    }

    Ok(())
}

fn init_fallback() -> keyring_core::Result<()> {
    if let Some(project_dirs) = ProjectDirs::from("com", "omni-oss", "omni") {
        let backing_file = project_dirs.config_dir().join("keystore");
        let keystore = fallback_store::Store::new_with_backing(&backing_file)?;
        keyring_core::set_default_store(keystore);
        tracing::trace!("initialized_omni_setup");
    } else {
        tracing::error!(
            "Failed to initialize fallback keyring store, no project config folder was determined"
        );
    }

    Ok(())
}

pub fn deinitialize() -> eyre::Result<()> {
    tracing::trace!("deinitializing_omni_setup");
    keyring_core::unset_default_store();
    tracing::trace!("deinitialized_omni_setup");
    Ok(())
}
