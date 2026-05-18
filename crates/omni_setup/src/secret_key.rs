use std::sync::Arc;

use base64::Engine;
use derive_new::new;
use keyring_core::api::CredentialStoreApi;
use rand::Rng;
use ring::digest;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

pub fn get_secret_key(
    service: &str,
    user: &str,
    store: Arc<dyn CredentialStoreApi>,
) -> Result<String, SecretKeyError> {
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
        log::debug!("secret key found in keyring");
        return Ok(result);
    }

    if let Err(error) = entry_result
        && !matches!(error, keyring_core::Error::NoEntry)
    {
        return Err(error.into());
    }

    let machine_id = machine_uid::get().map_err(|e| {
        SecretKeyErrorInner::MachineId(eyre::Report::msg(e.to_string()))
    })?;

    let mut random_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut random_bytes);
    let b64 = base64::engine::general_purpose::STANDARD.encode(random_bytes);

    let new_secret_key = format!("{service}-{user}-{machine_id}-{b64}");

    entry.set_password(&new_secret_key)?;

    log::debug!("secret key written to keyring");

    Ok(new_secret_key)
}

#[derive(Debug, thiserror::Error, new)]
#[error("error when trying to get secret key: {0}")]
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
        Self(inner.into())
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs, new)]
#[strum_discriminants(vis(pub), name(SecretKeyErrorKind))]
pub(crate) enum SecretKeyErrorInner {
    #[error(transparent)]
    Keyring(#[from] keyring_core::Error),

    #[error(transparent)]
    MachineId(eyre::Report),
}

#[cfg(test)]
mod tests {
    use keyring_core::mock;

    use super::*;

    #[test]
    fn test_get_secret_key() {
        let mock = mock::Store::new().unwrap();
        let key = get_secret_key("test-service", "test-user", mock)
            .expect("can't get secret key");
        assert!(!key.is_empty(), "key should not be empty");
    }
}
