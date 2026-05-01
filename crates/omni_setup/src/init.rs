pub fn initialize() -> eyre::Result<()> {
    trace::trace!("initializing_omni_setup");

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
        _ => {}
    }

    trace::trace!("initialized_omni_setup");
    Ok(())
}

pub fn deinitialize() -> eyre::Result<()> {
    trace::trace!("deinitializing_omni_setup");
    keyring_core::unset_default_store();
    trace::trace!("deinitialized_omni_setup");
    Ok(())
}
