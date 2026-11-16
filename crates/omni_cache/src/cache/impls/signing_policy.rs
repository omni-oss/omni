//! The effective signature policy of a run (RFC 0030, section 4.4): the
//! workspace's committed policy, which machine-local settings can only
//! tighten.

use std::collections::HashSet;

use omni_configurations::{
    RemoteCacheSettings, SigningKeyConfiguration, VerifyMode,
    WorkspaceRemoteCacheConfiguration,
};
use omni_remote_cache_client::{SigningError, TrustedKey};

/// A resolved verify mode: `auto` is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum EffectiveVerify {
    #[default]
    Off,
    Warn,
    Require,
}

impl EffectiveVerify {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Warn => "warn",
            Self::Require => "require",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SigningPolicyError {
    #[error("workspace trusted key {id:?}: {source}")]
    TrustedKey {
        id: String,
        #[source]
        source: SigningError,
    },
    #[error("the workspace trusts the key id {0} more than once")]
    DuplicateKey(String),
    #[error(
        "signing_key needs exactly one of private_key_env and private_key_file"
    )]
    AmbiguousSigningKey,
    #[error("signing_key id is not a valid key id")]
    InvalidSigningKeyId,
}

/// What a run enforces and signs with. The private key itself is not here: it
/// is loaded on demand.
#[derive(Debug, Clone, Default)]
pub struct SigningPolicy {
    pub verify: EffectiveVerify,
    /// The keys whose signatures are accepted, sorted by id.
    pub trusted: Vec<TrustedKey>,
    /// Where this machine's publisher key is read from.
    pub signing_key: Option<SigningKeyConfiguration>,
}

impl PartialEq for SigningPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.verify == other.verify
            && self.trusted_ids() == other.trusted_ids()
            && self.signing_key == other.signing_key
    }
}

impl Eq for SigningPolicy {}

impl SigningPolicy {
    pub fn trusted_ids(&self) -> Vec<String> {
        self.trusted.iter().map(|k| k.id().to_owned()).collect()
    }

    /// A policy that rejects every shared entry and refuses shared uploads:
    /// what a run uses when its configuration is invalid, so a mistake never
    /// weakens verification.
    pub fn fail_closed() -> Self {
        Self {
            verify: EffectiveVerify::Require,
            trusted: Vec::new(),
            signing_key: None,
        }
    }
}

/// Resolves the policy. Order matters and is fixed: the workspace `verify`
/// is resolved first (`auto` is `require` when the workspace trusts any key),
/// then the stricter of that and the local mode is taken, then a local
/// narrowing of the keys is applied. `auto` is never re-evaluated after
/// narrowing, so a local file cannot lower the policy by narrowing the key
/// set to nothing: `require` with no keys rejects every shared entry.
pub fn resolve_signing(
    workspace: &WorkspaceRemoteCacheConfiguration,
    local: &RemoteCacheSettings,
) -> Result<SigningPolicy, SigningPolicyError> {
    let mut trusted = Vec::new();
    let mut seen = HashSet::new();
    for key in &workspace.signing.trusted_keys {
        let parsed =
            TrustedKey::parse(&key.id, &key.public_key).map_err(|source| {
                SigningPolicyError::TrustedKey {
                    id: key.id.clone(),
                    source,
                }
            })?;
        if !seen.insert(parsed.id().to_owned()) {
            return Err(SigningPolicyError::DuplicateKey(key.id.clone()));
        }
        trusted.push(parsed);
    }

    let workspace_mode = match workspace.signing.verify {
        VerifyMode::Auto if trusted.is_empty() => EffectiveVerify::Off,
        VerifyMode::Auto | VerifyMode::Require => EffectiveVerify::Require,
        VerifyMode::Off => EffectiveVerify::Off,
        VerifyMode::Warn => EffectiveVerify::Warn,
    };
    // A local `auto` (or none) has no opinion: it never raises or lowers.
    let local_mode = match local.verify {
        None | Some(VerifyMode::Auto) | Some(VerifyMode::Off) => {
            EffectiveVerify::Off
        }
        Some(VerifyMode::Warn) => EffectiveVerify::Warn,
        Some(VerifyMode::Require) => EffectiveVerify::Require,
    };
    let verify = workspace_mode.max(local_mode);

    if let Some(narrowed) = &local.trusted_keys {
        for id in narrowed {
            if !seen.contains(id) {
                tracing::warn!(
                    %id,
                    "ignoring a local trusted key the workspace does not trust"
                );
            }
        }
        trusted.retain(|k| narrowed.iter().any(|id| id == k.id()));
    }
    trusted.sort_by(|a, b| a.id().cmp(b.id()));

    let signing_key = match &local.signing_key {
        None => None,
        Some(key) => {
            if key.private_key_env.is_some() == key.private_key_file.is_some() {
                return Err(SigningPolicyError::AmbiguousSigningKey);
            }
            if key.id.len() != 16
                || !key
                    .id
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err(SigningPolicyError::InvalidSigningKeyId);
            }
            Some(key.clone())
        }
    };

    Ok(SigningPolicy {
        verify,
        trusted,
        signing_key,
    })
}

#[cfg(test)]
mod tests {
    use omni_configurations::{
        TrustedKeyConfiguration, WorkspaceSigningConfiguration,
    };
    use omni_remote_cache_client::generate_keypair;

    use super::*;

    struct Fixture {
        a: TrustedKeyConfiguration,
        b: TrustedKeyConfiguration,
    }

    fn fixture() -> Fixture {
        let key = |name: &str| {
            let kp = generate_keypair().unwrap();
            TrustedKeyConfiguration {
                id: kp.id,
                name: Some(name.into()),
                public_key: kp.public_key,
            }
        };
        Fixture {
            a: key("a"),
            b: key("b"),
        }
    }

    fn workspace(
        keys: &[&TrustedKeyConfiguration],
        verify: VerifyMode,
    ) -> WorkspaceRemoteCacheConfiguration {
        WorkspaceRemoteCacheConfiguration {
            signing: WorkspaceSigningConfiguration {
                trusted_keys: keys.iter().map(|k| (*k).clone()).collect(),
                verify,
            },
        }
    }

    fn local(
        verify: Option<VerifyMode>,
        narrowed: Option<Vec<String>>,
    ) -> RemoteCacheSettings {
        RemoteCacheSettings {
            verify,
            trusted_keys: narrowed,
            ..Default::default()
        }
    }

    #[test]
    fn auto_turns_on_when_the_workspace_trusts_a_key() {
        let f = fixture();
        let none = resolve_signing(
            &workspace(&[], VerifyMode::Auto),
            &local(None, None),
        )
        .unwrap();
        assert_eq!(none.verify, EffectiveVerify::Off);
        let some = resolve_signing(
            &workspace(&[&f.a], VerifyMode::Auto),
            &local(None, None),
        )
        .unwrap();
        assert_eq!(some.verify, EffectiveVerify::Require);
        assert_eq!(some.trusted_ids(), vec![f.a.id.clone()]);
    }

    #[test]
    fn local_settings_only_tighten() {
        let f = fixture();
        // (workspace, local, expected)
        use EffectiveVerify::*;
        use VerifyMode as V;
        let table = [
            (V::Off, None, Off),
            (V::Off, Some(V::Off), Off),
            (V::Off, Some(V::Auto), Off),
            (V::Off, Some(V::Warn), Warn),
            (V::Off, Some(V::Require), Require),
            (V::Warn, Some(V::Off), Warn),
            (V::Warn, Some(V::Auto), Warn),
            (V::Warn, Some(V::Require), Require),
            (V::Require, Some(V::Off), Require),
            (V::Require, Some(V::Warn), Require),
            (V::Require, None, Require),
            (V::Auto, Some(V::Off), Require),
        ];
        // Without keys, `auto` is `off` and a local mode still tightens it.
        for (loc, expected) in [
            (None, Off),
            (Some(V::Warn), Warn),
            (Some(V::Require), Require),
        ] {
            let policy =
                resolve_signing(&workspace(&[], V::Auto), &local(loc, None))
                    .unwrap();
            assert_eq!(policy.verify, expected, "no keys + {loc:?}");
        }
        // A workspace `off` is honoured even when it lists keys.
        let off =
            resolve_signing(&workspace(&[&f.a], V::Off), &local(None, None))
                .unwrap();
        assert_eq!(off.verify, Off);
        for (ws, loc, expected) in table {
            let policy =
                resolve_signing(&workspace(&[&f.a], ws), &local(loc, None))
                    .unwrap();
            assert_eq!(policy.verify, expected, "{ws:?} + {loc:?}");
        }
    }

    #[test]
    fn narrowing_never_adds_a_key_and_never_lowers_the_mode() {
        let f = fixture();
        let ws = workspace(&[&f.a, &f.b], VerifyMode::Auto);
        let narrowed =
            resolve_signing(&ws, &local(None, Some(vec![f.a.id.clone()])))
                .unwrap();
        assert_eq!(narrowed.trusted_ids(), vec![f.a.id.clone()]);
        assert_eq!(narrowed.verify, EffectiveVerify::Require);
        // An id the workspace does not trust is ignored, not added.
        let stranger = resolve_signing(
            &ws,
            &local(None, Some(vec!["0123456789abcdef".into()])),
        )
        .unwrap();
        assert!(stranger.trusted.is_empty());
        // Narrowing to nothing leaves `require` in force: every shared entry misses.
        let empty = resolve_signing(&ws, &local(None, Some(vec![]))).unwrap();
        assert!(empty.trusted.is_empty());
        assert_eq!(empty.verify, EffectiveVerify::Require);
        // A workspace set resolved as `off` stays off even when narrowed.
        let off = resolve_signing(
            &workspace(&[], VerifyMode::Auto),
            &local(None, Some(vec![])),
        )
        .unwrap();
        assert_eq!(off.verify, EffectiveVerify::Off);
        // No local narrowing leaves the whole set.
        assert_eq!(
            resolve_signing(&ws, &local(None, None))
                .unwrap()
                .trusted
                .len(),
            2
        );
    }

    #[test]
    fn a_workspace_key_with_a_wrong_id_or_a_duplicate_is_an_error() {
        let f = fixture();
        let mut wrong = f.a.clone();
        wrong.id = f.b.id.clone();
        assert!(matches!(
            resolve_signing(
                &workspace(&[&wrong], VerifyMode::Auto),
                &local(None, None)
            ),
            Err(SigningPolicyError::TrustedKey { .. })
        ));
        assert!(matches!(
            resolve_signing(
                &workspace(&[&f.a, &f.a], VerifyMode::Auto),
                &local(None, None)
            ),
            Err(SigningPolicyError::DuplicateKey(_))
        ));
        let mut bad = f.a.clone();
        bad.public_key = "garbage".into();
        assert!(
            resolve_signing(
                &workspace(&[&bad], VerifyMode::Auto),
                &local(None, None)
            )
            .is_err()
        );
    }

    #[test]
    fn the_signing_key_reference_must_name_one_source() {
        let f = fixture();
        let ws = workspace(&[&f.a], VerifyMode::Auto);
        let with = |key: SigningKeyConfiguration| RemoteCacheSettings {
            signing_key: Some(key),
            ..Default::default()
        };
        let env = |id: &str| SigningKeyConfiguration {
            id: id.into(),
            private_key_env: Some("K".into()),
            private_key_file: None,
        };
        assert!(
            resolve_signing(&ws, &with(env(&f.a.id)))
                .unwrap()
                .signing_key
                .is_some()
        );
        assert_eq!(
            resolve_signing(&ws, &with(env("nope"))).unwrap_err(),
            SigningPolicyError::InvalidSigningKeyId
        );
        let both = SigningKeyConfiguration {
            private_key_file: Some("k".into()),
            ..env(&f.a.id)
        };
        let neither = SigningKeyConfiguration {
            private_key_env: None,
            ..env(&f.a.id)
        };
        for bad in [both, neither] {
            assert_eq!(
                resolve_signing(&ws, &with(bad)).unwrap_err(),
                SigningPolicyError::AmbiguousSigningKey
            );
        }
    }

    #[test]
    fn a_policy_that_fails_closed_rejects_everything() {
        let p = SigningPolicy::fail_closed();
        assert_eq!(p.verify, EffectiveVerify::Require);
        assert!(p.trusted.is_empty() && p.signing_key.is_none());
    }
}
