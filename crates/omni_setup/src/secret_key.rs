use std::sync::Arc;

use base64::Engine;
use derive_new::new;
use keyring_core::api::{CredentialStore, CredentialStoreApi};
use rand::Rng;
use ring::digest;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

pub(crate) fn default_store() -> Result<Arc<CredentialStore>, SecretKeyError> {
    require_store(keyring_core::get_default_store())
}

fn require_store(
    store: Option<Arc<CredentialStore>>,
) -> Result<Arc<CredentialStore>, SecretKeyError> {
    store.ok_or_else(|| SecretKeyErrorInner::Keyring.into())
}

pub fn get_secret_key(
    service: &str,
    user: &str,
    store: Arc<dyn CredentialStoreApi>,
) -> Result<String, SecretKeyError> {
    let fallback = store
        .as_any()
        .downcast_ref::<crate::fallback_store::Store>();
    let _creation = fallback
        .map(|store| store.lock_key_creation())
        .transpose()?;
    let service = if service.is_empty() {
        "omni-remote-cache-client"
    } else {
        service
    };
    let user = if user.is_empty() { "default" } else { user };

    let service = digest::digest(&digest::SHA256, service.as_bytes());
    let service =
        base64::engine::general_purpose::STANDARD.encode(service.as_ref());
    let user = digest::digest(&digest::SHA256, user.as_bytes());
    let user = base64::engine::general_purpose::STANDARD.encode(user.as_ref());

    let entry = store.build(&service, &user, None)?;

    let entry_result = entry.get_password();

    if let Ok(result) = entry_result {
        tracing::debug!("secret key found in keyring");
        return Ok(result);
    }

    if let Err(error) = entry_result
        && !matches!(error, keyring_core::Error::NoEntry)
    {
        return Err(error.into());
    }

    let machine_id =
        machine_uid::get().map_err(|_| SecretKeyErrorInner::MachineId)?;

    let mut random_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut random_bytes);
    let b64 = base64::engine::general_purpose::STANDARD.encode(random_bytes);

    let new_secret_key = format!("{service}-{user}-{machine_id}-{b64}");

    entry.set_password(&new_secret_key)?;
    if let Some(store) = fallback
        && let Err(error) = store.save()
    {
        // Don't let a subsequent call use the unpublished key in memory.
        let _ = entry.delete_credential();
        return Err(error.into());
    }

    tracing::debug!("secret key written to keyring");

    Ok(new_secret_key)
}

#[derive(Debug, thiserror::Error, new, omni_diagnostic::Diagnostic)]
#[error("error when trying to get secret key: {0}")]
#[diagnostic(forward)]
pub struct SecretKeyError(pub(crate) SecretKeyErrorInner);

impl SecretKeyError {
    #[allow(unused)]
    pub fn kind(&self) -> SecretKeyErrorKind {
        self.0.discriminant()
    }
}

impl<T: Into<SecretKeyErrorInner>> From<T> for SecretKeyError {
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
#[strum_discriminants(vis(pub), name(SecretKeyErrorKind))]
pub(crate) enum SecretKeyErrorInner {
    /// The credential store could not read or save the encryption key.
    #[error(
        "could not access the credential store for the remote-cache encryption key"
    )]
    #[diagnostic(
        code = "remote_cache.keyring_unavailable",
        audience = environment,
        hint = "make sure the current account can access its credential store, then retry"
    )]
    Keyring,

    /// Creating an encryption key requires a readable machine identifier.
    #[error(
        "could not read the machine identifier for the remote-cache encryption key"
    )]
    #[diagnostic(
        code = "remote_cache.machine_id_unavailable",
        audience = environment,
        hint = "check that this host has a machine identifier readable by the current account, then retry"
    )]
    MachineId,
}

impl From<keyring_core::Error> for SecretKeyErrorInner {
    fn from(_: keyring_core::Error) -> Self {
        // Credential-store errors can include account identifiers or data
        // supplied by the backend. Do not retain them in the report.
        Self::Keyring
    }
}

#[cfg(test)]
mod tests {
    use keyring_core::mock;

    use super::*;

    #[test]
    fn missing_default_store_returns_a_diagnostic() {
        let result = require_store(None);
        let Err(error) = result else {
            panic!("a missing store must not be accepted");
        };
        let report = eyre::Report::new(error);
        let diagnostic = omni_diagnostic::find(&report).unwrap();
        assert_eq!(diagnostic.code(), "remote_cache.keyring_unavailable");
    }

    #[test]
    fn test_get_secret_key() {
        let mock = mock::Store::new().unwrap();
        let key = get_secret_key("test-service", "test-user", mock)
            .expect("can't get secret key");
        assert!(!key.is_empty(), "key should not be empty");
    }

    #[test]
    fn fallback_key_is_readable_from_disk_before_store_drops() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new-config-dir").join("keystore");
        let store =
            crate::fallback_store::Store::open_for_remote_cache(&path).unwrap();
        let stale =
            crate::fallback_store::Store::open_for_remote_cache(&path).unwrap();
        let key =
            get_secret_key("test-service", "test-user", store.clone()).unwrap();

        drop(stale);
        assert!(path.is_file());
        let reloaded =
            crate::fallback_store::Store::open_for_remote_cache(&path).unwrap();
        let reloaded_key =
            get_secret_key("test-service", "test-user", reloaded).unwrap();
        assert!(
            key == reloaded_key,
            "reloaded key differed from the persisted key"
        );
        drop(store);
    }
}
