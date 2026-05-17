use bon::Builder;

#[derive(Builder, Default)]
pub struct InitConfig {
    #[builder(default)]
    pub use_fallback_store: bool,
}

pub fn initialize(config: InitConfig) -> eyre::Result<()> {
    trace::trace!("initializing_omni_setup");

    if config.use_fallback_store {
        return Ok(init_fallback()?);
    }

    let result = try {
        cfg_select! {
            target_os = "macos" => {
                let store = apple_native_keyring_store::keychain::Store::new()?;
                keyring_core::set_default_store(store);
            },
            target_os = "windows" => {
                let store = windows_native_keyring_store::store::Store::new()?;
                keyring_core::set_default_store(store);
            },
            target_os = "linux" => {
                let store = zbus_secret_service_keyring_store::Store::new()?;
                keyring_core::set_default_store(store);
            },
            _ => {
                return erye::erye!("Unsupported platform, omni_setup only supports macOS, Windows and Linux");
            }
        }
    };

    match result {
        Ok(_) => {
            trace::trace!("initialized_omni_setup");
            Ok(())
        }
        Err(err) => match err {
            keyring_core::Error::PlatformFailure(error) => {
                // normalize the text
                let text_err = error.to_string().to_lowercase();

                if text_err.contains("the name org.freedesktop.secrets was not provided by any .service files") {
                    init_fallback()?;
                    Ok(())
                } else {
                    Err(keyring_core::Error::PlatformFailure(error).into())
                }
            }
            error => Err(error.into()),
        },
    }
}

fn init_fallback() -> eyre::Result<()> {
    let config = db_keystore::DbKeyStoreConfig::default();
    let keystore = db_keystore::DbKeyStore::new(config)?;

    keyring_core::set_default_store(keystore);

    Ok(())
}

pub fn deinitialize() -> eyre::Result<()> {
    trace::trace!("deinitializing_omni_setup");
    keyring_core::unset_default_store();
    trace::trace!("deinitialized_omni_setup");
    Ok(())
}
