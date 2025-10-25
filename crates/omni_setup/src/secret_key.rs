use crate::util::env;
use base64::Engine;
use derive_new::new;
use keyring::Entry;
use rand::RngCore;
use strum::{EnumDiscriminants, EnumIs, IntoDiscriminant};

pub fn get_secret_key() -> Result<String, SecretKeyError> {
    let service = env!("OMNI_SECRET_SERVICE_NAME", "omni")?;
    let user = env!("OMNI_SECRET_USER_NAME", "omni")?;

    let entry = Entry::new(&service, &user)?;

    let entry_result = entry.get_password();

    if let Ok(result) = entry_result {
        trace::debug!("secret key found in keyring");
        return Ok(result);
    }

    if let Err(error) = entry_result
        && !matches!(error, keyring::Error::NoEntry)
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

    trace::debug!("secret key written to keyring");

    Ok(new_secret_key)
}

#[derive(Debug, thiserror::Error, new)]
#[error("failed to setup remote caching: {inner}")]
pub struct SecretKeyError {
    kind: SecretKeyErrorKind,
    inner: SecretKeyErrorInner,
}

impl SecretKeyError {
    #[allow(unused)]
    pub fn kind(&self) -> SecretKeyErrorKind {
        self.kind
    }
}

impl<T: Into<SecretKeyErrorInner>> From<T> for SecretKeyError {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self {
            kind: inner.discriminant(),
            inner: inner.into(),
        }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants, EnumIs, new)]
#[strum_discriminants(vis(pub), name(SecretKeyErrorKind))]
enum SecretKeyErrorInner {
    #[error(transparent)]
    Keyring(#[from] keyring::Error),

    #[error(transparent)]
    MachineId(eyre::Report),

    #[error(transparent)]
    Var(#[from] std::env::VarError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_secret_key() {
        let key = get_secret_key().expect("can't get secret key");
        assert!(!key.is_empty(), "key should not be empty");
    }
}
