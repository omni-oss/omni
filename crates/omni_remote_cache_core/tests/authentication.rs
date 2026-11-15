use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use omni_remote_cache_core::security::{
    AuthError, Authenticator, AuthenticatorChain, Caller, CredentialId,
    CredentialKind, Resource, Scope, Scopes,
};

const ALL: [Scope; 5] = [
    Scope::Read,
    Scope::Write,
    Scope::List,
    Scope::Delete,
    Scope::Publish,
];

fn resource(env: &str) -> Resource {
    Resource {
        tenant: "t".into(),
        org: "o".into(),
        ws: "w".into(),
        env: env.into(),
    }
}

fn identity(scopes: Scopes) -> Caller {
    Caller::new(
        "ci-principal".into(),
        CredentialId::new(
            CredentialKind::Custom("oidc".into()),
            "opaque-subject-id".into(),
        )
        .unwrap(),
        scopes,
        Some("pr-42".into()),
    )
    .unwrap()
}

enum Answer {
    Unknown,
    Recognized(Caller),
    Unavailable,
}

type Observations = Arc<Mutex<Vec<(String, Resource, Scopes)>>>;

struct Member {
    name: &'static str,
    calls: Observations,
    answer: Answer,
}

#[async_trait]
impl Authenticator for Member {
    async fn authenticate(
        &self,
        credential: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        // Assert forwarding without storing the raw credential in observations.
        assert_eq!(credential, "test-token");
        self.calls.lock().unwrap().push((
            self.name.into(),
            resource.clone(),
            wanted,
        ));
        match &self.answer {
            Answer::Unknown => Ok(None),
            Answer::Recognized(caller) => Ok(Some(caller.clone())),
            Answer::Unavailable => {
                Err(AuthError::Backend(eyre::eyre!("test backend unavailable")))
            }
        }
    }
}

fn member(
    name: &'static str,
    calls: &Observations,
    answer: Answer,
) -> Arc<dyn Authenticator> {
    Arc::new(Member {
        name,
        calls: calls.clone(),
        answer,
    })
}

#[tokio::test]
async fn empty_and_unrecognized_chains_return_none() {
    let target = resource("e");
    assert!(
        AuthenticatorChain::default()
            .authenticate("test-token", &target, Scope::Read.into())
            .await
            .unwrap()
            .is_none()
    );
    let calls = Observations::default();
    let chain = AuthenticatorChain::new(vec![
        member("one", &calls, Answer::Unknown),
        member("two", &calls, Answer::Unknown),
    ]);
    assert!(
        chain
            .authenticate("test-token", &target, Scope::Read.into())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [
            ("one".into(), target.clone(), Scope::Read.into()),
            ("two".into(), target, Scope::Read.into()),
        ]
    );
}

#[tokio::test]
async fn first_match_preserves_identity_and_only_requested_grants() {
    let calls = Observations::default();
    let chain = AuthenticatorChain::new(vec![
        member("unknown", &calls, Answer::Unknown),
        member("match", &calls, Answer::Recognized(identity(ALL.into()))),
        member("must-not-run", &calls, Answer::Unavailable),
    ]);
    let wanted = Scopes::from([Scope::Read, Scope::List]);
    let target = resource("prod");
    let caller = chain
        .authenticate("test-token", &target, wanted)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(caller.principal(), "ci-principal");
    assert_eq!(caller.credential().kind().as_str(), "oidc");
    assert_eq!(caller.credential().id(), "opaque-subject-id");
    assert_eq!(caller.scope_pin(), Some("pr-42"));
    assert_eq!(caller.scopes(), wanted);
    for scope in ALL {
        assert_eq!(caller.can(scope), wanted.contains(scope), "{scope:?}");
    }
    assert_eq!(
        *calls.lock().unwrap(),
        [
            ("unknown".into(), target.clone(), wanted),
            ("match".into(), target, wanted),
        ]
    );
}

#[tokio::test]
async fn denial_and_empty_wanted_never_fall_through_to_a_more_privileged_member()
 {
    for (granted, wanted) in [
        (Scopes::default(), Scope::Publish.into()),
        (Scope::Read.into(), Scope::Write.into()),
        (ALL.into(), Scopes::default()),
    ] {
        let calls = Observations::default();
        let chain = AuthenticatorChain::new(vec![
            member("denied", &calls, Answer::Recognized(identity(granted))),
            member(
                "privileged",
                &calls,
                Answer::Recognized(identity(ALL.into())),
            ),
        ]);
        let caller = chain
            .authenticate("test-token", &resource("e"), wanted)
            .await
            .unwrap()
            .expect("recognized even with no effective grants");
        assert_eq!(caller.scopes(), Scopes::default());
        assert_eq!(calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn errors_stop_the_chain_rather_than_falling_through() {
    let calls = Observations::default();
    let chain = AuthenticatorChain::new(vec![
        member("offline", &calls, Answer::Unavailable),
        member("fallback", &calls, Answer::Recognized(identity(ALL.into()))),
    ]);
    assert!(matches!(
        chain
            .authenticate("test-token", &resource("e"), ALL.into())
            .await,
        Err(AuthError::Backend(_))
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
}

struct ResourceScoped;

#[async_trait]
impl Authenticator for ResourceScoped {
    async fn authenticate(
        &self,
        _: &str,
        resource: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        let granted = if resource == &crate::resource("dev") {
            Scopes::from([Scope::Read, Scope::Write])
        } else {
            Scope::Read.into()
        };
        Ok(Some(identity(granted.intersection(wanted))))
    }
}

#[tokio::test]
async fn each_request_is_authenticated_for_its_own_resource_and_wanted_set() {
    let chain = AuthenticatorChain::new(vec![Arc::new(ResourceScoped)]);
    for (target, scope, permitted) in [
        (resource("dev"), Scope::Write, true),
        (resource("prod"), Scope::Write, false),
        (resource("prod"), Scope::Read, true),
        (resource("dev"), Scope::Publish, false),
        (resource("dev"), Scope::Write, true),
    ] {
        let caller = chain
            .authenticate("test-token", &target, scope.into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(caller.can(scope), permitted, "{target:?}, {scope:?}");
    }
}

#[test]
fn scopes_have_no_implicit_grants_and_use_existing_wire_names() {
    let names = [
        "read:artifacts",
        "write:artifacts",
        "list:artifacts",
        "delete:artifacts",
        "publish:artifacts",
    ];
    for (scope, name) in ALL.into_iter().zip(names) {
        assert_eq!(scope.as_str(), name);
        assert!(!Scopes::default().contains(scope));
        for other in ALL {
            assert_eq!(Scopes::from(scope).contains(other), scope == other);
        }
    }
    let legacy_all =
        Scopes::from([Scope::Read, Scope::Write, Scope::List, Scope::Delete]);
    assert!(!legacy_all.contains(Scope::Publish));
    assert_eq!(Scopes::from([Scope::Read, Scope::Read]), Scope::Read.into());
    // Equal sets built in different orders compare identically for cache keys.
    assert_eq!(
        Scopes::from([Scope::Read, Scope::Write]),
        Scopes::from([Scope::Write, Scope::Read])
    );
}

#[test]
fn identity_text_is_nonempty_control_free_and_bounded_in_utf8_bytes() {
    let credential = identity(Scopes::default()).credential().clone();
    for bad in ["".into(), "a".repeat(257), "é".repeat(129), "x\ny".into()] {
        assert!(matches!(
            Caller::new(
                bad.clone(),
                credential.clone(),
                Scopes::default(),
                None
            ),
            Err(AuthError::InvalidIdentity)
        ));
        assert!(
            Caller::new(
                "ok".into(),
                credential.clone(),
                Scopes::default(),
                Some(bad.clone())
            )
            .is_err()
        );
        assert!(
            CredentialId::new(CredentialKind::ApiKey, bad.clone()).is_err()
        );
        assert!(
            CredentialId::new(CredentialKind::Custom(bad), "opaque-id".into())
                .is_err()
        );
    }
    for good in ["a".repeat(256), "é".repeat(128)] {
        assert!(
            Caller::new(
                good.clone(),
                credential.clone(),
                Scopes::default(),
                Some(good)
            )
            .is_ok()
        );
    }
    assert!(
        CredentialId::new(
            CredentialKind::Custom("api_key".into()),
            "opaque-id".into()
        )
        .is_err(),
        "custom kinds cannot impersonate a built-in kind"
    );
}

#[test]
fn backend_error_display_does_not_expose_the_backend_report() {
    let error = AuthError::Backend(eyre::eyre!("private backend detail"));
    assert_eq!(error.to_string(), "authentication backend unavailable");
}
