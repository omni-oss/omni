//! RFC 0030 slice 4: scopes and namespaces through the real routes.
//!
//! The token is `principal|scopes|pin`: scopes are letters (`r`ead, `w`rite,
//! `p`ublish, `l`ist, `d`elete) and the pin is `-` for none.

use std::sync::Arc;

use async_trait::async_trait;
use axum_test::{TestRequest, TestServer};
use bytes::Bytes;
use http::{Method, StatusCode};
use omni_remote_cache_core::{
    config::Configuration,
    data::{
        environments::DynEnvironmentRepository,
        organizations::DynOrganizationRepository, tenants::DynTenantRepository,
        workspaces::DynWorkspaceRepository,
    },
    entry::{EntryRead, read_entry},
    namespace::segment,
    providers::{ConfigBasedDependencyProvider, DependencyProvider},
    security::{
        AuthError, Authenticator, AuthenticatorChain, Caller, CredentialId,
        CredentialKind, Resource, Scope, Scopes,
    },
    services::DynValidationService,
    state::{ServiceInfo, ServiceState},
};
use omni_remote_cache_storage_core::{Container, RemoteCacheStorageBackend};
use omni_remote_cache_storage_memory::InMemoryBackend;
use sha2::{Digest as _, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

struct Tokens;

#[async_trait]
impl Authenticator for Tokens {
    async fn authenticate(
        &self,
        credential: &str,
        _: &Resource,
        wanted: Scopes,
    ) -> Result<Option<Caller>, AuthError> {
        let mut parts = credential.split('|');
        let (Some(principal), Some(letters), Some(pin), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Ok(None);
        };
        let held: Scopes = letters
            .chars()
            .map(|c| match c {
                'r' => Scope::Read,
                'w' => Scope::Write,
                'p' => Scope::Publish,
                'l' => Scope::List,
                _ => Scope::Delete,
            })
            .collect();
        Caller::new(
            principal.into(),
            CredentialId::new(
                CredentialKind::Custom("t".into()),
                principal.into(),
            )?,
            held.intersection(wanted),
            (pin != "-").then(|| pin.to_string()),
        )
        .map(Some)
    }
}

struct Provider {
    inner: ConfigBasedDependencyProvider,
}

impl DependencyProvider for Provider {
    fn authenticator(&self) -> Arc<dyn Authenticator> {
        Arc::new(AuthenticatorChain::new(vec![Arc::new(Tokens)]))
    }
    fn workspace_repository(&self) -> DynWorkspaceRepository {
        self.inner.workspace_repository()
    }
    fn organization_repository(&self) -> DynOrganizationRepository {
        self.inner.organization_repository()
    }
    fn environment_repository(&self) -> DynEnvironmentRepository {
        self.inner.environment_repository()
    }
    fn tenant_repository(&self) -> DynTenantRepository {
        self.inner.tenant_repository()
    }
    fn validation_service(&self) -> DynValidationService {
        self.inner.validation_service()
    }
}

fn server() -> (TestServer, Arc<InMemoryBackend>) {
    let config = Configuration::from_inline(
        serde_json::json!({
            "tenants": { "t": { "organizations": { "o": { "workspaces": {
                "w": { "environments": { "e": {} } }
            } } } } },
            "security": { "api_keys": {} }
        })
        .to_string(),
    )
    .unwrap();
    let backend = Arc::new(InMemoryBackend::new());
    let provider = Arc::new(Provider {
        inner: ConfigBasedDependencyProvider::new(
            Arc::new(config.to_in_memory_database()),
            Arc::new(config.security.api_keys),
        ),
    });
    let state = ServiceState::new(
        backend.clone(),
        Arc::new(ServiceInfo::new(
            "test",
            "0",
            "",
            false,
            Some("/api".into()),
        )),
        provider,
    );
    let router = omni_remote_cache_service::routes::root::build_router(
        &omni_remote_cache_core::routes::root::RouterConfig::new(
            Some("/api".into()),
            false,
        ),
    )
    .with_state(state);
    (TestServer::new(router), backend)
}

fn url(digest: &str, extra: &str) -> String {
    format!("/api/v1/artifacts/{digest}?org=o&ws=w&env=e{extra}")
}

fn list_url(extra: &str) -> String {
    format!("/api/v1/artifacts?org=o&ws=w&env=e{extra}")
}

fn with(req: TestRequest, token: &str) -> TestRequest {
    req.add_header("Authorization", format!("Bearer {token}"))
        .add_header("X-OMNI-TENANT", "t")
}

fn put(
    s: &TestServer,
    token: &str,
    extra: &str,
    digest: &str,
    body: &str,
) -> TestRequest {
    with(s.put(&url(digest, extra)), token)
        .add_header("X-Omni-Content-Sha256", sha256_hex(body.as_bytes()))
        .bytes(body.to_owned().into())
}

fn get(s: &TestServer, token: &str, extra: &str, digest: &str) -> TestRequest {
    with(s.get(&url(digest, extra)), token)
}

async fn status(req: TestRequest) -> StatusCode {
    req.await.status_code()
}

const SCOPE_SETS: [&str; 10] =
    ["r", "w", "p", "l", "d", "rw", "rp", "wp", "rwp", "rwpld"];

fn has(set: &str, c: char) -> bool {
    set.contains(c)
}

#[tokio::test]
async fn every_scope_set_against_every_route_and_namespace() {
    let (s, _) = server();
    for set in SCOPE_SETS {
        let token = format!("alice|{set}|-");
        let write_iso = has(set, 'w') || has(set, 'p');

        let expect = |name: &str, allowed: bool, got: StatusCode| {
            assert_eq!(
                got == StatusCode::FORBIDDEN,
                !allowed,
                "{name} with {set}: got {got}"
            );
            if allowed {
                assert!(
                    [200, 204, 404, 409].contains(&got.as_u16()),
                    "{name} with {set}: allowed but got {got}"
                );
            }
        };
        expect(
            "PUT isolated",
            write_iso,
            status(put(
                &s,
                &token,
                "&target=isolated",
                &format!("a{}", set.len()),
                "x",
            ))
            .await,
        );
        expect(
            "PUT default is isolated",
            write_iso,
            status(put(&s, &token, "", &format!("b{}", set.len()), "x")).await,
        );
        expect(
            "PUT shared",
            has(set, 'p'),
            status(put(
                &s,
                &token,
                "&target=shared",
                &format!("c{}", set.len()),
                "x",
            ))
            .await,
        );
        expect(
            "GET shared",
            has(set, 'r'),
            status(get(&s, &token, "", "zz")).await,
        );
        expect(
            "GET isolated",
            has(set, 'r') && write_iso,
            status(get(&s, &token, "&namespace=isolated", "zz")).await,
        );
        expect(
            "HEAD shared",
            has(set, 'r'),
            status(with(s.method(Method::HEAD, &url("zz", "")), &token)).await,
        );
        expect(
            "HEAD isolated",
            has(set, 'r') && write_iso,
            status(with(
                s.method(Method::HEAD, &url("zz", "&namespace=isolated")),
                &token,
            ))
            .await,
        );
        expect(
            "LIST shared",
            has(set, 'l'),
            status(with(s.get(&list_url("")), &token)).await,
        );
        expect(
            "LIST isolated",
            has(set, 'l'),
            status(with(s.get(&list_url("&namespace=isolated")), &token)).await,
        );
        expect(
            "DELETE shared",
            has(set, 'd'),
            status(with(s.delete(&url("zz", "")), &token)).await,
        );
        expect(
            "DELETE isolated",
            has(set, 'd'),
            status(with(s.delete(&url("zz", "&namespace=isolated")), &token))
                .await,
        );
    }
}

#[tokio::test]
async fn a_read_only_key_can_never_write_and_all_without_publish_cannot_publish()
 {
    let (s, backend) = server();
    for target in ["", "&target=isolated", "&target=shared"] {
        assert_eq!(
            status(put(&s, "ro|r|-", target, "d1", "x")).await,
            StatusCode::FORBIDDEN
        );
    }
    // write and delete and list and read, but no publish
    assert_eq!(
        status(put(&s, "all|rwld|-", "&target=shared", "d1", "x")).await,
        StatusCode::FORBIDDEN
    );
    assert!(
        backend
            .list(&Container::parse("shared/t/o/w/e").unwrap())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn unknown_parameter_values_are_bad_requests() {
    let (s, _) = server();
    let t = "a|rwpld|-";
    for extra in ["&target=other", "&target=", "&target=Shared"] {
        assert_eq!(
            status(put(&s, t, extra, "d1", "x")).await,
            StatusCode::BAD_REQUEST,
            "PUT {extra}"
        );
    }
    for extra in ["&namespace=other", "&namespace=", "&namespace=Shared"] {
        assert_eq!(
            status(get(&s, t, extra, "d1")).await,
            StatusCode::BAD_REQUEST,
            "GET {extra}"
        );
        assert_eq!(
            status(with(s.method(Method::HEAD, &url("d1", extra)), t)).await,
            StatusCode::BAD_REQUEST,
            "HEAD {extra}"
        );
        assert_eq!(
            status(with(s.delete(&url("d1", extra)), t)).await,
            StatusCode::BAD_REQUEST,
            "DELETE {extra}"
        );
        assert_eq!(
            status(with(s.get(&list_url(extra)), t)).await,
            StatusCode::BAD_REQUEST,
            "LIST {extra}"
        );
    }
}

#[tokio::test]
async fn principals_and_scopes_are_invisible_to_each_other() {
    let (s, _) = server();
    assert_eq!(
        status(put(&s, "alice|rw|-", "&scope=one", "d1", "alice-one")).await,
        StatusCode::NO_CONTENT
    );
    let own =
        get(&s, "alice|rw|-", "&namespace=isolated&scope=one", "d1").await;
    own.assert_status_ok();
    assert_eq!(own.as_bytes().as_ref(), b"alice-one");

    // Another scope of the same principal, and other principals, miss.
    for (token, scope) in [
        ("alice|rw|-", "two"),
        ("bob|rw|-", "one"),
        ("Alice|rw|-", "one"),
        ("ALICE|rw|-", "one"),
        ("alice.|rw|-", "one"),
    ] {
        assert_eq!(
            status(get(
                &s,
                token,
                &format!("&namespace=isolated&scope={scope}"),
                "d1"
            ))
            .await,
            StatusCode::NOT_FOUND,
            "{token} {scope}"
        );
    }
    // A principal that spells alice's segment as text is a different principal.
    let spelled = segment("Alice");
    assert!(spelled.starts_with("h-"));
    assert_eq!(
        status(put(&s, "Alice|rw|-", "&scope=one", "d2", "upper")).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(get(
            &s,
            &format!("{spelled}|rw|-"),
            "&namespace=isolated&scope=one",
            "d2"
        ))
        .await,
        StatusCode::NOT_FOUND
    );
    // Isolated entries are never visible in the shared namespace.
    assert_eq!(
        status(get(&s, "alice|rw|-", "", "d1")).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_pin_overrides_the_client_scope_and_an_invalid_one_fails_closed() {
    let (s, backend) = server();
    assert_eq!(
        status(put(&s, "ci|rw|pr-7", "&scope=attacker", "d1", "x")).await,
        StatusCode::NO_CONTENT
    );
    let pinned =
        Container::parse(format!("isolated/t/o/w/e/ci/{}", segment("pr-7")))
            .unwrap();
    assert!(backend.exists(&pinned, "d1").await.unwrap());
    let other = Container::parse(format!(
        "isolated/t/o/w/e/ci/{}",
        segment("attacker")
    ))
    .unwrap();
    assert!(!backend.exists(&other, "d1").await.unwrap());

    for bad in ["../x", "a/b", ""] {
        assert_eq!(
            status(put(&s, &format!("ci|rw|{bad}"), "", "d3", "x")).await,
            if bad.is_empty() {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            },
            "pin {bad:?}"
        );
    }
}

#[tokio::test]
async fn a_client_scope_must_be_a_label_and_is_ignored_for_shared() {
    let (s, backend) = server();
    for bad in ["..", "a%2Fb", "-x", "a%20b", "%2E%2E"] {
        assert_eq!(
            status(put(&s, "a|rw|-", &format!("&scope={bad}"), "d1", "x"))
                .await,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    assert_eq!(
        status(put(&s, "a|rwp|-", "&target=shared&scope=..", "d1", "x")).await,
        StatusCode::NO_CONTENT
    );
    assert!(
        backend
            .exists(&Container::parse("shared/t/o/w/e").unwrap(), "d1")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn the_same_digest_coexists_per_namespace_and_conflicts_per_namespace() {
    let (s, _) = server();
    assert_eq!(
        status(put(&s, "a|rwp|-", "&target=shared", "d1", "shared-bytes"))
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(put(
            &s,
            "a|rwp|-",
            "&target=isolated",
            "d1",
            "private-bytes"
        ))
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(put(&s, "a|rwp|-", "&target=shared", "d1", "other")).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        status(put(&s, "a|rwp|-", "&target=isolated", "d1", "other")).await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        get(&s, "a|rwp|-", "", "d1").await.as_bytes().as_ref(),
        b"shared-bytes"
    );
    assert_eq!(
        get(&s, "a|rwp|-", "&namespace=isolated", "d1")
            .await
            .as_bytes()
            .as_ref(),
        b"private-bytes"
    );
    // Another principal reads the published entry, not the private one.
    assert_eq!(
        get(&s, "b|r|-", "", "d1").await.as_bytes().as_ref(),
        b"shared-bytes"
    );
}

#[tokio::test]
async fn trailers_record_the_real_namespace_and_scope() {
    let (s, backend) = server();
    put(&s, "a|rwp|-", "&target=shared", "d1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    put(&s, "a|rwp|-", "&scope=pr-1", "d1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let shared = read_entry(
        backend.as_ref(),
        &Container::parse("shared/t/o/w/e").unwrap(),
        "d1",
    )
    .await
    .unwrap();
    let iso = read_entry(
        backend.as_ref(),
        &Container::parse(format!("isolated/t/o/w/e/a/{}", segment("pr-1")))
            .unwrap(),
        "d1",
    )
    .await
    .unwrap();
    let (EntryRead::Valid(shared), EntryRead::Valid(iso)) = (shared, iso)
    else {
        panic!("both entries are sealed");
    };
    assert_eq!(shared.parsed.trailer.namespace, "shared");
    assert_eq!(shared.parsed.trailer.scope, None);
    assert_eq!(iso.parsed.trailer.namespace, "isolated");
    assert_eq!(iso.parsed.trailer.scope.as_deref(), Some("pr-1"));
    assert_eq!(iso.parsed.trailer.writer_principal, "a");
}

#[tokio::test]
async fn bytes_copied_between_prefixes_or_from_the_legacy_container_are_misses()
{
    let (s, backend) = server();
    put(&s, "alice|rwp|-", "&scope=one", "d1", "secret")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    let src = Container::parse("isolated/t/o/w/e/alice/one").unwrap();
    let raw: Bytes = backend.get(&src, "d1").await.unwrap().unwrap();

    for (dest, token, extra) in [
        // into the shared namespace: its trailer says isolated
        ("shared/t/o/w/e", "r|r|-", ""),
        // into another principal's and another scope's prefix
        (
            "isolated/t/o/w/e/bob/one",
            "bob|rw|-",
            "&namespace=isolated&scope=one",
        ),
        (
            "isolated/t/o/w/e/alice/two",
            "alice|rw|-",
            "&namespace=isolated&scope=two",
        ),
        // the layout that existed before namespaces
        ("t/o/w/e", "r|r|-", ""),
    ] {
        backend
            .save_if_absent(&Container::parse(dest).unwrap(), "d1", raw.clone())
            .await
            .unwrap();
        assert_eq!(
            status(get(&s, token, extra, "d1")).await,
            StatusCode::NOT_FOUND,
            "{dest}"
        );
        assert_eq!(
            status(with(s.method(Method::HEAD, &url("d1", extra)), token))
                .await,
            StatusCode::NOT_FOUND,
            "{dest}"
        );
    }
    // Copied into shared but now claiming shared is impossible without the
    // server: an isolated entry read back from its own place still works.
    get(&s, "alice|rw|-", "&namespace=isolated&scope=one", "d1")
        .await
        .assert_status_ok();
}

#[tokio::test]
async fn lists_cover_one_namespace_and_isolated_lists_name_the_scope() {
    let (s, _) = server();
    put(&s, "a|rwpl|-", "&target=shared", "s1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    put(&s, "a|rwpl|-", "&scope=one", "i1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    put(&s, "a|rwpl|-", "&scope=two", "i2", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    put(&s, "b|rwpl|-", "&scope=one", "j1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);

    let shared: serde_json::Value =
        with(s.get(&list_url("")), "a|rwpl|-").await.json();
    let digests: Vec<_> = shared["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["digest"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(digests, ["s1"]);
    assert!(shared["data"][0].get("scope").is_none());

    let iso: serde_json::Value =
        with(s.get(&list_url("&namespace=isolated")), "a|rwpl|-")
            .await
            .json();
    let mut rows: Vec<_> = iso["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["scope"].as_str().unwrap().to_owned(),
                i["digest"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [("one".into(), "i1".into()), ("two".into(), "i2".into())]
    );
}

#[tokio::test]
async fn delete_reaches_only_the_namespace_it_names_and_the_callers_own() {
    let (s, backend) = server();
    put(&s, "a|rwpd|-", "&target=shared", "d1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    put(&s, "a|rwpd|-", "", "d1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    put(&s, "b|rwpd|-", "", "d1", "x")
        .await
        .assert_status(StatusCode::NO_CONTENT);

    with(s.delete(&url("d1", "&namespace=isolated")), "a|rwpd|-")
        .await
        .assert_status(StatusCode::NO_CONTENT);
    assert!(
        !backend
            .exists(
                &Container::parse("isolated/t/o/w/e/a/default").unwrap(),
                "d1"
            )
            .await
            .unwrap()
    );
    assert!(
        backend
            .exists(&Container::parse("shared/t/o/w/e").unwrap(), "d1")
            .await
            .unwrap()
    );
    assert!(
        backend
            .exists(
                &Container::parse("isolated/t/o/w/e/b/default").unwrap(),
                "d1"
            )
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn the_probe_reports_the_principal_segment_and_the_effective_scope() {
    let (s, _) = server();
    let probe = |token: &'static str, extra: &'static str| {
        let s = &s;
        async move { with(s.method(Method::HEAD, &list_url(extra)), token).await }
    };
    let r = probe("Alice|rw|-", "").await;
    r.assert_status(StatusCode::NO_CONTENT);
    assert_eq!(r.header("x-omni-principal-id"), segment("Alice").as_str());
    assert_eq!(r.header("x-omni-scope"), "default");
    assert_eq!(
        probe("alice|rw|-", "&scope=pr-9")
            .await
            .header("x-omni-scope"),
        "pr-9"
    );
    assert_eq!(
        probe("alice|rw|ci-1", "&scope=pr-9")
            .await
            .header("x-omni-scope"),
        "ci-1"
    );
    probe("alice|rw|-", "&scope=..")
        .await
        .assert_status(StatusCode::BAD_REQUEST);
}
