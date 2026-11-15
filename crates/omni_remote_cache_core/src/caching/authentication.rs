use std::sync::Arc;

use async_trait::async_trait;
use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;

use super::{CacheConfiguration, TtlCache};
use crate::security::{
    AuthError, Authenticator, AuthenticatorChain, Caller, Resource, Scopes,
};

pub(super) struct AuthenticationCache {
    entries: TtlCache<Option<Caller>>,
    // Per-cache random pepper, not the stable public credential-ID secret.
    // Never serialize or debug-print this key or retain raw credentials.
    key: [u8; 32],
}

impl AuthenticationCache {
    pub(super) fn new(config: CacheConfiguration) -> Option<Self> {
        Self::with_rng(config, getrandom::fill)
    }

    fn with_rng(
        config: CacheConfiguration,
        fill: impl FnOnce(&mut [u8]) -> Result<(), getrandom::Error>,
    ) -> Option<Self> {
        let mut key = [0; 32];
        if fill(&mut key).is_err() {
            // No unkeyed fallback: authenticate on every request instead.
            tracing::warn!(
                "authentication cache disabled: secure randomness unavailable"
            );
            return None;
        }
        Some(Self {
            entries: TtlCache::new(config),
            key,
        })
    }

    fn cache_key(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key)
            .expect("HMAC accepts a 32-byte key");
        // Length prefixes distinguish components even when they contain ':' or
        // scope-like strings. A canonical scope order makes sets order-independent.
        let parts = [
            "omni-authentication-cache-v1",
            credential,
            &resource.tenant,
            &resource.org,
            &resource.ws,
            &resource.env,
        ];
        for part in parts
            .into_iter()
            .chain(wanted.iter().map(|scope| scope.as_str()))
        {
            mac.update(&(part.len() as u64).to_be_bytes());
            mac.update(part.as_bytes());
        }
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

pub(super) struct CachedAuthenticator {
    inner: AuthenticatorChain,
    cache: Option<Arc<AuthenticationCache>>,
}

impl CachedAuthenticator {
    pub(super) fn new(
        inner: Arc<dyn Authenticator>,
        cache: Option<Arc<AuthenticationCache>>,
    ) -> Self {
        // Narrow member grants before caching, even for third-party providers.
        Self {
            inner: AuthenticatorChain::new(vec![inner]),
            cache,
        }
    }
}

#[async_trait]
impl Authenticator for CachedAuthenticator {
    async fn authenticate(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        let call = self.inner.authenticate(credential, resource, wanted);
        match &self.cache {
            Some(cache) => {
                cache
                    .entries
                    .get_or_try(
                        cache.cache_key(credential, resource, wanted),
                        call,
                        // Recognized callers (even no grants) use the security TTL.
                        // Unknown credentials use the optional negative TTL.
                        Option::is_some,
                    )
                    .await
            }
            None => call.await,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    };

    use super::*;
    use crate::security::{CredentialId, CredentialKind, Scope};

    struct Mutable {
        answer: Mutex<Option<Caller>>,
        unavailable: AtomicBool,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl Authenticator for Mutable {
        async fn authenticate(
            &self,
            _: &str,
            _: &Resource,
            _: Scopes,
        ) -> Result<Option<Caller>, AuthError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.unavailable.load(Ordering::SeqCst) {
                return Err(AuthError::Backend(eyre::eyre!("offline")));
            }
            Ok(self.answer.lock().unwrap().clone())
        }
    }

    fn identity(principal: &str, scopes: Scopes) -> Caller {
        Caller::new(
            principal.into(),
            CredentialId::new(
                CredentialKind::Custom("oidc".into()),
                "opaque-id".into(),
            )
            .unwrap(),
            scopes,
            Some("pr-42".into()),
        )
        .unwrap()
    }

    fn backend() -> Arc<Mutable> {
        Arc::new(Mutable {
            answer: Mutex::new(Some(identity(
                "owner",
                [Scope::Read, Scope::Write].into(),
            ))),
            unavailable: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
        })
    }

    fn resource() -> Resource {
        Resource {
            tenant: "t".into(),
            org: "o".into(),
            ws: "w".into(),
            env: "e".into(),
        }
    }

    fn config(negative: Option<u64>) -> CacheConfiguration {
        CacheConfiguration {
            ttl_secs: 10,
            max_entries: 100,
            negative_ttl_secs: negative,
        }
    }

    fn fixed_clock() -> i128 {
        0
    }

    fn cache(config: CacheConfiguration) -> Arc<AuthenticationCache> {
        Arc::new(AuthenticationCache {
            key: [1; 32],
            entries: TtlCache::with_clock(config, fixed_clock),
        })
    }

    #[test]
    fn keys_are_peppered_full_length_unambiguous_and_canonical() {
        let a = cache(config(None));
        let b = AuthenticationCache {
            key: [2; 32],
            entries: TtlCache::with_clock(config(None), fixed_clock),
        };
        let target = resource();
        let key = a.cache_key("private-token", &target, Scope::Read.into());
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!key.contains("private-token"));
        assert_ne!(
            key,
            b.cache_key("private-token", &target, Scope::Read.into())
        );
        assert_ne!(
            a.cache_key(
                "x",
                &Resource {
                    tenant: "a:b".into(),
                    org: "c".into(),
                    ..target.clone()
                },
                Scopes::default()
            ),
            a.cache_key(
                "x",
                &Resource {
                    tenant: "a".into(),
                    org: "b:c".into(),
                    ..target.clone()
                },
                Scopes::default()
            ),
        );
        assert_eq!(
            a.cache_key("x", &target, [Scope::Read, Scope::Write].into()),
            a.cache_key("x", &target, [Scope::Write, Scope::Read].into()),
        );
    }

    #[test]
    fn randomness_failure_disables_the_cache_instead_of_using_an_unkeyed_hash()
    {
        assert!(
            AuthenticationCache::with_rng(config(None), |_| Err(
                getrandom::Error::UNSUPPORTED
            ))
            .is_none()
        );
    }

    #[tokio::test]
    async fn cache_never_shares_credentials_resources_or_wanted_sets() {
        let inner = backend();
        let shared = cache(config(None));
        let auth =
            CachedAuthenticator::new(inner.clone(), Some(shared.clone()));
        let target = resource();
        for _ in 0..2 {
            let caller = auth
                .authenticate("key", &target, Scope::Read.into())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                caller.scopes(),
                Scope::Read.into(),
                "extra member grants are narrowed before caching"
            );
            assert_eq!(caller.scope_pin(), Some("pr-42"));
        }
        assert_eq!(inner.calls.load(Ordering::SeqCst), 1);
        for other in [
            Resource {
                tenant: "other".into(),
                ..target.clone()
            },
            Resource {
                org: "other".into(),
                ..target.clone()
            },
            Resource {
                ws: "other".into(),
                ..target.clone()
            },
            Resource {
                env: "other".into(),
                ..target.clone()
            },
        ] {
            auth.authenticate("key", &other, Scope::Read.into())
                .await
                .unwrap();
        }
        auth.authenticate("other-key", &target, Scope::Read.into())
            .await
            .unwrap();
        auth.authenticate("key", &target, Scope::Write.into())
            .await
            .unwrap();
        assert_eq!(inner.calls.load(Ordering::SeqCst), 7);
        // A rebuilt request-scoped decorator uses the same state.
        let rebuilt = CachedAuthenticator::new(inner.clone(), Some(shared));
        rebuilt
            .authenticate("key", &target, Scope::Read.into())
            .await
            .unwrap();
        assert_eq!(inner.calls.load(Ordering::SeqCst), 7);
    }

    #[tokio::test]
    async fn unknowns_errors_and_disabled_cache_do_not_create_positive_hits() {
        let inner = backend();
        *inner.answer.lock().unwrap() = None;
        let shared = cache(config(None));
        let auth =
            CachedAuthenticator::new(inner.clone(), Some(shared.clone()));
        for _ in 0..2 {
            assert!(
                auth.authenticate("k", &resource(), Scope::Read.into())
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(inner.calls.load(Ordering::SeqCst), 2);
        assert!(shared.entries.is_empty());
        inner.unavailable.store(true, Ordering::SeqCst);
        for _ in 0..2 {
            assert!(
                auth.authenticate("k", &resource(), Scope::Read.into())
                    .await
                    .is_err()
            );
        }
        assert_eq!(inner.calls.load(Ordering::SeqCst), 4);
        assert!(shared.entries.is_empty());
        inner.unavailable.store(false, Ordering::SeqCst);
        *inner.answer.lock().unwrap() =
            Some(identity("owner", Scope::Read.into()));
        let uncached = CachedAuthenticator::new(inner.clone(), None);
        for _ in 0..2 {
            uncached
                .authenticate("k", &resource(), Scope::Read.into())
                .await
                .unwrap();
        }
        assert_eq!(inner.calls.load(Ordering::SeqCst), 6);
    }

    #[tokio::test]
    async fn capacity_and_zero_ttl_remain_bounded() {
        let inner = backend();
        let mut cfg = config(None);
        cfg.max_entries = 1;
        let shared = cache(cfg);
        let auth =
            CachedAuthenticator::new(inner.clone(), Some(shared.clone()));
        for token in ["a", "b", "a"] {
            auth.authenticate(token, &resource(), Scope::Read.into())
                .await
                .unwrap();
        }
        assert_eq!(shared.entries.len(), 1);
        assert_eq!(inner.calls.load(Ordering::SeqCst), 3);
        let mut cfg = config(None);
        cfg.ttl_secs = 0;
        let shared = cache(cfg);
        let auth =
            CachedAuthenticator::new(inner.clone(), Some(shared.clone()));
        for _ in 0..2 {
            auth.authenticate("k", &resource(), Scope::Read.into())
                .await
                .unwrap();
        }
        assert!(shared.entries.is_empty());
        assert_eq!(inner.calls.load(Ordering::SeqCst), 5);
    }

    // A single deterministic-clock test owns this clock.
    static NOW: AtomicI64 = AtomicI64::new(0);
    fn clock() -> i128 {
        NOW.load(Ordering::SeqCst) as i128 * 1_000_000_000
    }

    #[tokio::test]
    async fn revocation_principal_changes_and_negative_caching_end_at_ttl_boundaries()
     {
        NOW.store(0, Ordering::SeqCst);
        let inner = backend();
        let shared = Arc::new(AuthenticationCache {
            key: [1; 32],
            entries: TtlCache::with_clock(config(Some(2)), clock),
        });
        let auth = CachedAuthenticator::new(inner.clone(), Some(shared));
        let wanted = [Scope::Read, Scope::Write].into();
        auth.authenticate("k", &resource(), wanted).await.unwrap();
        *inner.answer.lock().unwrap() =
            Some(identity("new-owner", Scope::Read.into()));
        NOW.store(9, Ordering::SeqCst);
        let old = auth
            .authenticate("k", &resource(), wanted)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(old.principal(), "owner");
        assert!(old.can(Scope::Write));
        NOW.store(10, Ordering::SeqCst);
        let fresh = auth
            .authenticate("k", &resource(), wanted)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fresh.principal(), "new-owner");
        assert!(!fresh.can(Scope::Write));
        *inner.answer.lock().unwrap() = None; // key removed
        NOW.store(19, Ordering::SeqCst);
        assert!(
            auth.authenticate("k", &resource(), wanted)
                .await
                .unwrap()
                .is_some()
        );
        NOW.store(20, Ordering::SeqCst);
        assert!(
            auth.authenticate("k", &resource(), wanted)
                .await
                .unwrap()
                .is_none()
        );
        *inner.answer.lock().unwrap() = Some(identity("restored", wanted));
        NOW.store(21, Ordering::SeqCst);
        assert!(
            auth.authenticate("k", &resource(), wanted)
                .await
                .unwrap()
                .is_none()
        );
        NOW.store(22, Ordering::SeqCst);
        assert_eq!(
            auth.authenticate("k", &resource(), wanted)
                .await
                .unwrap()
                .unwrap()
                .principal(),
            "restored"
        );

        // A recognized denial is a positive identity result, not an unknown
        // credential. Its cached zero grants must not grow before expiry.
        *inner.answer.lock().unwrap() =
            Some(identity("denied", Scopes::default()));
        NOW.store(32, Ordering::SeqCst);
        assert!(
            !auth
                .authenticate("k", &resource(), wanted)
                .await
                .unwrap()
                .unwrap()
                .can(Scope::Read)
        );
        *inner.answer.lock().unwrap() = Some(identity("granted", wanted));
        NOW.store(41, Ordering::SeqCst);
        assert!(
            !auth
                .authenticate("k", &resource(), wanted)
                .await
                .unwrap()
                .unwrap()
                .can(Scope::Read)
        );
        NOW.store(42, Ordering::SeqCst);
        assert!(
            auth.authenticate("k", &resource(), wanted)
                .await
                .unwrap()
                .unwrap()
                .can(Scope::Read)
        );
    }
}
