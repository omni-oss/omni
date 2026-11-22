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

    const DIR_ENV: &str = "OMNI_KEYSTORE_TEST_DIR";
    const ROLE_ENV: &str = "OMNI_KEYSTORE_TEST_ROLE";

    fn fallback_key(path: &std::path::Path) -> String {
        let store =
            crate::fallback_store::Store::open_for_remote_cache(path).unwrap();
        get_secret_key("svc", "usr", store).unwrap()
    }

    fn aead_key(secret: &str) -> ring::aead::LessSafeKey {
        let digest = digest::digest(&digest::SHA256, secret.as_bytes());
        ring::aead::LessSafeKey::new(
            ring::aead::UnboundKey::new(
                &ring::aead::AES_256_GCM,
                digest.as_ref(),
            )
            .unwrap(),
        )
    }

    const NONCE: [u8; 12] = [7; 12];
    const PLAIN: &[u8] = b"remote cache config";

    /// Runs only as a child of the multi-process tests below.
    #[test]
    fn keystore_child_process() {
        let Ok(dir) = std::env::var(DIR_ENV) else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let path = dir.join("config").join("keystore");
        let key = if std::env::var(ROLE_ENV).as_deref() == Ok("stale") {
            // Open (and snapshot) the store, then wait for the parent to
            // save a key before asking for one.
            let store =
                crate::fallback_store::Store::open_for_remote_cache(&path)
                    .unwrap();
            std::fs::write(dir.join("ready"), b"").unwrap();
            while !dir.join("go").exists() {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            get_secret_key("svc", "usr", store).unwrap()
        } else {
            fallback_key(&path)
        };
        let out = match std::env::var(ROLE_ENV).as_deref() {
            Ok("seal") => {
                let mut data = PLAIN.to_vec();
                aead_key(&key)
                    .seal_in_place_append_tag(
                        ring::aead::Nonce::assume_unique_for_key(NONCE),
                        ring::aead::Aad::empty(),
                        &mut data,
                    )
                    .unwrap();
                data
            }
            _ => key.into_bytes(),
        };
        let name = format!("out-{}", std::process::id());
        std::fs::write(dir.join(name), out).unwrap();
    }

    fn spawn_child(dir: &std::path::Path, role: &str) -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "secret_key::tests::keystore_child_process"])
            .env(DIR_ENV, dir)
            .env(ROLE_ENV, role)
            .spawn()
            .unwrap()
    }

    #[test]
    fn concurrent_threads_and_processes_agree_on_one_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config").join("keystore");
        let children: Vec<_> =
            (0..6).map(|_| spawn_child(dir.path(), "key")).collect();
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || fallback_key(&path))
            })
            .collect();
        let mut keys: Vec<String> =
            threads.into_iter().map(|t| t.join().unwrap()).collect();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().starts_with("out-") {
                keys.push(
                    String::from_utf8(std::fs::read(entry.path()).unwrap())
                        .unwrap(),
                );
            }
        }
        assert_eq!(keys.len(), 12);
        assert!(keys.iter().all(|k| *k == keys[0]), "keys diverged");
        assert_eq!(fallback_key(&path), keys[0]);
    }

    #[test]
    fn fresh_config_dir_key_decrypts_in_a_second_process() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = spawn_child(dir.path(), "seal");
        assert!(child.wait().unwrap().success());
        let sealed = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name().unwrap().to_string_lossy().starts_with("out-")
            })
            .unwrap();
        let mut data = std::fs::read(sealed).unwrap();
        let key = fallback_key(&dir.path().join("config").join("keystore"));
        let plain = aead_key(&key)
            .open_in_place(
                ring::aead::Nonce::assume_unique_for_key(NONCE),
                ring::aead::Aad::empty(),
                &mut data,
            )
            .unwrap();
        assert_eq!(plain, PLAIN);
    }

    fn wait_for(path: &std::path::Path) {
        for _ in 0..1000 {
            if path.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {}", path.display());
    }

    fn child_output(dir: &std::path::Path, child: u32) -> String {
        String::from_utf8(
            std::fs::read(dir.join(format!("out-{child}"))).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn stale_snapshot_in_another_process_adopts_the_saved_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config").join("keystore");
        let mut child = spawn_child(dir.path(), "stale");
        wait_for(&dir.path().join("ready"));
        let saved = fallback_key(&path);
        std::fs::write(dir.path().join("go"), b"").unwrap();
        assert!(child.wait().unwrap().success());
        assert_eq!(child_output(dir.path(), child.id()), saved);
    }

    #[test]
    fn another_process_waits_while_the_keystore_lock_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config").join("keystore");
        let store =
            crate::fallback_store::Store::open_for_remote_cache(&path).unwrap();
        let held = store.lock_key_creation().unwrap();
        let mut child = spawn_child(dir.path(), "key");
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(
            child.try_wait().unwrap().is_none(),
            "child ignored the lock"
        );
        drop(held);
        assert!(child.wait().unwrap().success());
    }
}
