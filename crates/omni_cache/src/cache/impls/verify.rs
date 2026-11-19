//! Verifying shared entries before extraction, and signing shared uploads
//! (RFC 0030, section 4.5).

use std::{path::Path, sync::Arc};

use omni_configurations::SigningKeyConfiguration;
use omni_remote_cache_client::{
    RemoteArtifact, SigningHandle, signed_message, verify,
};
use sha2::{Digest as _, Sha256};

use super::{
    hybrid::EnabledRemoteConfig,
    signing_policy::{EffectiveVerify, SigningPolicy},
};

/// The outcome of checking one shared entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Not checked: the policy is `off`.
    Skipped,
    /// Signed by this trusted key id.
    Verified(String),
    /// Not accepted. The text never contains signature bytes.
    Failed(String),
}

/// The SHA-256 of `bytes`, hashed off the async worker threads.
pub(crate) async fn sha256_of(
    bytes: bytes::Bytes,
) -> Result<[u8; 32], tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || Sha256::digest(&bytes).into()).await
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Checks a downloaded shared entry against the policy. The hash the message
/// binds is the one computed here from the bytes in hand, never the header,
/// so the same buffer that is verified is the one that is extracted. The
/// `X-Omni-Content-Sha256` header only has to agree with it.
pub(crate) async fn verify_shared(
    conf: &EnabledRemoteConfig,
    digest: &str,
    artifact: &RemoteArtifact,
) -> Verdict {
    if conf.signing.verify == EffectiveVerify::Off {
        return Verdict::Skipped;
    }
    if artifact.ambiguous_provenance {
        return Verdict::Failed(
            "the server sent a provenance header twice".into(),
        );
    }
    let Ok(actual) = sha256_of(artifact.bytes.clone()).await else {
        return Verdict::Failed("hashing the download failed".into());
    };
    if artifact.sha256.as_deref() != Some(hex(&actual).as_str()) {
        return Verdict::Failed(
            "the download does not match the hash the server reported".into(),
        );
    }
    let message = signed_message(
        &conf.tenant_code,
        &conf.organization_code,
        &conf.workspace_code,
        conf.env_code(),
        digest,
        &actual,
    );
    match verify(
        &conf.signing.trusted,
        artifact.signature.as_deref(),
        &message,
    ) {
        Ok(id) => Verdict::Verified(id),
        Err(rejection) => Verdict::Failed(rejection.to_string()),
    }
}

/// Reads the private key a publisher signs with. It comes only from the process
/// environment or a file named in the machine-local config (never from omni's
/// `.env` loading or any committed file), is checked against the configured id
/// and is held in a zeroized handle.
pub(crate) fn load_signing_key(
    reference: &SigningKeyConfiguration,
) -> Result<Arc<SigningHandle>, String> {
    let text =
        if let Some(name) = &reference.private_key_env {
            zeroize::Zeroizing::new(std::env::var(name).map_err(|_| {
                format!("the environment variable {name} is not set")
            })?)
        } else if let Some(path) = &reference.private_key_file {
            warn_if_open(path);
            zeroize::Zeroizing::new(std::fs::read_to_string(path).map_err(
                |e| format!("cannot read the signing key file: {e}"),
            )?)
        } else {
            return Err("no signing key source is configured".into());
        };
    let handle = SigningHandle::from_base64(&text).map_err(|_| {
        "the signing key is not a valid base64 Ed25519 key".to_string()
    })?;
    if handle.id() != reference.id {
        return Err(format!(
            "the signing key is {} but the configuration names {}",
            handle.id(),
            reference.id
        ));
    }
    Ok(Arc::new(handle))
}

#[cfg(unix)]
fn warn_if_open(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    if let Ok(meta) = std::fs::metadata(path)
        && meta.permissions().mode() & 0o077 != 0
    {
        tracing::warn!(
            path = %path.display(),
            "the signing key file is readable by other users; restrict it to mode 0600"
        );
    }
}

#[cfg(not(unix))]
fn warn_if_open(_: &Path) {}

/// What a shared upload may do under `policy`.
#[derive(Debug)]
pub(crate) enum SigningPlan {
    /// Upload unsigned: no key is configured and none is trusted.
    Unsigned,
    /// Sign every shared upload with this key.
    Sign(Arc<SigningHandle>),
    /// Do not upload, for this reason.
    Refuse(String),
}

pub(crate) fn plan_shared_uploads(policy: &SigningPolicy) -> SigningPlan {
    match &policy.signing_key {
        Some(reference) => match load_signing_key(reference) {
            Ok(handle) => SigningPlan::Sign(handle),
            Err(why) => SigningPlan::Refuse(why),
        },
        // `require` means keys are trusted somewhere, even when a local
        // narrowing (or an invalid configuration) left this machine with none:
        // it must not publish what no verifier could accept.
        None if !policy.trusted.is_empty()
            || policy.verify == EffectiveVerify::Require =>
        {
            SigningPlan::Refuse(
                "signatures are required but this machine has no signing key"
                    .into(),
            )
        }
        None => SigningPlan::Unsigned,
    }
}

#[cfg(test)]
mod tests {
    use omni_remote_cache_client::{TrustedKey, generate_keypair};

    use super::*;

    fn config(
        verify_mode: EffectiveVerify,
        trusted: Vec<TrustedKey>,
    ) -> EnabledRemoteConfig {
        let mut conf = EnabledRemoteConfig::new(
            "http://h",
            "k",
            "t",
            "o",
            "w",
            Some("e".to_string()),
        );
        conf.signing = Box::new(SigningPolicy {
            verify: verify_mode,
            trusted,
            signing_key: None,
        });
        conf
    }

    struct Publisher {
        handle: SigningHandle,
        trusted: TrustedKey,
    }

    fn publisher() -> Publisher {
        let kp = generate_keypair().unwrap();
        Publisher {
            handle: SigningHandle::from_base64(&kp.private_key).unwrap(),
            trusted: TrustedKey::parse(&kp.id, &kp.public_key).unwrap(),
        }
    }

    fn signed(
        p: &Publisher,
        bytes: &[u8],
        digest: &str,
        place: (&str, &str, &str, &str),
    ) -> RemoteArtifact {
        let hash: [u8; 32] = Sha256::digest(bytes).into();
        let message =
            signed_message(place.0, place.1, place.2, place.3, digest, &hash);
        RemoteArtifact {
            bytes: bytes::Bytes::copy_from_slice(bytes),
            sha256: Some(hex(&hash)),
            signature: Some(p.handle.sign(&message)),
            ambiguous_provenance: false,
        }
    }

    const HERE: (&str, &str, &str, &str) = ("t", "o", "w", "e");

    #[tokio::test]
    async fn a_good_entry_verifies_and_names_the_key() {
        let p = publisher();
        let conf = config(EffectiveVerify::Require, vec![p.trusted.clone()]);
        let artifact = signed(&p, b"archive", "d1", HERE);
        assert_eq!(
            verify_shared(&conf, "d1", &artifact).await,
            Verdict::Verified(p.trusted.id().to_owned())
        );
    }

    #[tokio::test]
    async fn off_skips_everything() {
        let conf = config(EffectiveVerify::Off, vec![]);
        let junk = RemoteArtifact {
            bytes: bytes::Bytes::from_static(b"x"),
            sha256: None,
            signature: None,
            ambiguous_provenance: true,
        };
        assert_eq!(verify_shared(&conf, "d1", &junk).await, Verdict::Skipped);
    }

    #[tokio::test]
    async fn every_kind_of_tampering_fails() {
        let p = publisher();
        let other = publisher();
        let conf = config(EffectiveVerify::Require, vec![p.trusted.clone()]);
        let good = signed(&p, b"archive", "d1", HERE);
        let failed = |v: Verdict| matches!(v, Verdict::Failed(_));

        // Different bytes under the same signature and header.
        let mut swapped = good.clone();
        swapped.bytes = bytes::Bytes::from_static(b"evil archive");
        assert!(failed(verify_shared(&conf, "d1", &swapped).await));
        // A server that also rewrites the hash header: the signature covers
        // the hash of the bytes, so it still fails.
        let mut rehashed = swapped.clone();
        rehashed.sha256 = Some(hex(&Sha256::digest(b"evil archive")));
        assert!(failed(verify_shared(&conf, "d1", &rehashed).await));
        // Missing or ambiguous provenance.
        let mut no_hash = good.clone();
        no_hash.sha256 = None;
        assert!(failed(verify_shared(&conf, "d1", &no_hash).await));
        let mut twice = good.clone();
        twice.ambiguous_provenance = true;
        assert!(failed(verify_shared(&conf, "d1", &twice).await));
        let mut unsigned = good.clone();
        unsigned.signature = None;
        assert_eq!(
            verify_shared(&conf, "d1", &unsigned).await,
            Verdict::Failed("the entry is not signed".into())
        );
        // Replayed under another digest, tenant, org, workspace or environment.
        assert!(failed(verify_shared(&conf, "d2", &good).await));
        for (field, value) in [(0, "t2"), (1, "o2"), (2, "w2"), (3, "e2")] {
            let mut c =
                config(EffectiveVerify::Require, vec![p.trusted.clone()]);
            match field {
                0 => c.tenant_code = value.into(),
                1 => c.organization_code = value.into(),
                2 => c.workspace_code = value.into(),
                _ => c.environment_code = Some(value.into()),
            }
            assert!(failed(verify_shared(&c, "d1", &good).await), "{field}");
        }
        // A key the workspace does not trust.
        let by_other = signed(&other, b"archive", "d1", HERE);
        assert_eq!(
            verify_shared(&conf, "d1", &by_other).await,
            Verdict::Failed(
                "the entry is signed by a key that is not trusted".into()
            )
        );
        // No trusted keys under require rejects everything.
        let empty = config(EffectiveVerify::Require, vec![]);
        assert!(failed(verify_shared(&empty, "d1", &good).await));
    }

    #[tokio::test]
    async fn the_environment_default_matches_the_url() {
        // With no environment code the client uses `default` everywhere.
        let p = publisher();
        let mut conf =
            config(EffectiveVerify::Require, vec![p.trusted.clone()]);
        conf.environment_code = None;
        let artifact = signed(&p, b"a", "d1", ("t", "o", "w", "default"));
        assert!(matches!(
            verify_shared(&conf, "d1", &artifact).await,
            Verdict::Verified(_)
        ));
    }

    #[test]
    fn the_key_loads_only_when_it_is_the_configured_one() {
        let kp = generate_keypair().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("key");
        std::fs::write(&file, format!("{}\n", *kp.private_key)).unwrap();
        let reference = |id: &str| SigningKeyConfiguration {
            id: id.into(),
            private_key_env: None,
            private_key_file: Some(file.clone()),
        };
        assert_eq!(load_signing_key(&reference(&kp.id)).unwrap().id(), kp.id);
        let wrong = generate_keypair().unwrap();
        assert!(load_signing_key(&reference(&wrong.id)).is_err());
        std::fs::write(&file, "not a key").unwrap();
        assert!(load_signing_key(&reference(&kp.id)).is_err());
        let missing = SigningKeyConfiguration {
            id: kp.id.clone(),
            private_key_env: Some("OMNI_TEST_KEY_THAT_IS_NOT_SET".into()),
            private_key_file: None,
        };
        assert!(load_signing_key(&missing).is_err());
    }

    #[test]
    fn shared_uploads_follow_the_policy() {
        let p = publisher();
        let none = SigningPolicy::default();
        assert!(matches!(plan_shared_uploads(&none), SigningPlan::Unsigned));
        let trusting = SigningPolicy {
            verify: EffectiveVerify::Require,
            trusted: vec![p.trusted.clone()],
            signing_key: None,
        };
        assert!(matches!(
            plan_shared_uploads(&trusting),
            SigningPlan::Refuse(_)
        ));
        let broken = SigningPolicy {
            signing_key: Some(SigningKeyConfiguration {
                id: p.trusted.id().into(),
                private_key_env: Some("OMNI_TEST_KEY_THAT_IS_NOT_SET".into()),
                private_key_file: None,
            }),
            ..SigningPolicy::default()
        };
        assert!(matches!(
            plan_shared_uploads(&broken),
            SigningPlan::Refuse(_)
        ));
        // An invalid configuration fails closed, and so does a narrowing that
        // left no keys under `require`.
        assert!(matches!(
            plan_shared_uploads(&SigningPolicy::fail_closed()),
            SigningPlan::Refuse(_)
        ));
        let narrowed = SigningPolicy {
            verify: EffectiveVerify::Require,
            ..SigningPolicy::default()
        };
        assert!(matches!(
            plan_shared_uploads(&narrowed),
            SigningPlan::Refuse(_)
        ));
        // `warn` with nothing trusted does not stop a publisher.
        let warn = SigningPolicy {
            verify: EffectiveVerify::Warn,
            ..SigningPolicy::default()
        };
        assert!(matches!(plan_shared_uploads(&warn), SigningPlan::Unsigned));
    }
}
