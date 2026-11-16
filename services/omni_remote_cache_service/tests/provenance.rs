//! Entries are stored sealed: the artifact, then a server-written trailer that a
//! client cannot influence (RFC 0030, section 4.3).

use std::sync::Arc;

use axum_test::{TestRequest, TestServer};
use bytes::Bytes;
use bytesize::ByteSize;
use http::StatusCode;
use maps::unordered_map;
use omni_remote_cache_service::{
    args::{BackendType, ConfigType, RouterArgs, ServeArgs},
    config::{
        All, AllOrSpecificConfiguration, ApiKeyConfiguration, Configuration,
        EnvironmentConfiguration, OrganizationConfiguration,
        SecurityConfiguration, TenantConfiguration, WorkspaceConfiguration,
    },
    entry::{FOOTER_LEN, MAGIC, Trailer},
    routes::root::RouterConfig,
    state::build_state,
};
use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, PrefixPage, ReadStream,
    RemoteCacheStorageBackend, SaveOutcome, WriteStream, error::Error,
};
use omni_remote_cache_storage_local::LocalDiskCacheBackend;
use sha2::{Digest as _, Sha256};

const QUERY: &str = "org=o&ws=w&env=e&namespace=isolated&target=isolated";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn config() -> Configuration {
    Configuration {
        tenants: unordered_map!("t".to_string() => TenantConfiguration {
            description: None,
            display_name: None,
            organizations: unordered_map!("o".to_string() => OrganizationConfiguration {
                description: None,
                display_name: None,
                workspaces: unordered_map!("w".to_string() => WorkspaceConfiguration {
                    description: None,
                    display_name: None,
                    environments: unordered_map!("e".to_string() => EnvironmentConfiguration {
                        description: None,
                        display_name: None,
                    }),
                }),
            }),
        }),
        security: SecurityConfiguration {
            api_key_id_secret: None,
            api_keys: unordered_map!("k".to_string() => ApiKeyConfiguration {
                principal: None,
                description: None,
                enabled: true,
                expires_at: None,
                workspaces: AllOrSpecificConfiguration::new_all(All::new_all()),
                environments: AllOrSpecificConfiguration::new_all(All::new_all()),
                organizations: AllOrSpecificConfiguration::new_all(All::new_all()),
                scopes: AllOrSpecificConfiguration::new_all(All::new_all()),
                tenants: AllOrSpecificConfiguration::new_all(All::new_all()),
            }),
        },
    }
}

async fn router(
    root: &std::path::Path,
) -> (axum::Router, Arc<LocalDiskCacheBackend>) {
    let args = ServeArgs::new(
        "".to_string(),
        Some(serde_json::to_string(&config()).unwrap()),
        Some(ConfigType::Inline),
        false,
        None,
        None,
        None,
        BackendType::InMemory,
        Some(RouterArgs::new(Some("/api".to_string()), false)),
        None,
    );
    let mut state = build_state(&args).await.unwrap();
    let backend = Arc::new(LocalDiskCacheBackend::new(root));
    state.storage_backend = backend.clone();
    let router = omni_remote_cache_service::routes::root::build_router(
        &RouterConfig::from(args.routes.clone().unwrap()),
    )
    .with_state(state);
    (router, backend)
}

async fn server(
    root: &std::path::Path,
) -> (TestServer, Arc<LocalDiskCacheBackend>) {
    let (router, backend) = router(root).await;
    (TestServer::new(router), backend)
}

fn container() -> Container {
    Container::parse(format!(
        "isolated/t/o/w/e/{}/default",
        omni_remote_cache_service::hooks::api_key_id("k")
    ))
    .unwrap()
}

fn path(digest: &str) -> String {
    format!("/api/v1/artifacts/{digest}?{QUERY}")
}

fn authed(request: TestRequest) -> TestRequest {
    request
        .add_header("Authorization", "Bearer k")
        .add_header("X-OMNI-TENANT", "t")
}

async fn put(server: &TestServer, digest: &str, artifact: &[u8]) -> StatusCode {
    authed(server.put(&path(digest)))
        .add_header("X-Omni-Content-Sha256", sha256_hex(artifact))
        .bytes(Bytes::copy_from_slice(artifact))
        .await
        .status_code()
}

fn files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = vec![];
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    found
}

async fn stored(
    backend: &LocalDiskCacheBackend,
    digest: &str,
) -> Option<Bytes> {
    backend.get(&container(), digest).await.unwrap()
}

#[tokio::test]
async fn a_stored_entry_is_the_artifact_then_server_authored_provenance() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    let artifact = b"the archive bytes";
    let before = time::OffsetDateTime::now_utc();
    assert_eq!(put(&server, "d1", artifact).await, StatusCode::NO_CONTENT);
    let after = time::OffsetDateTime::now_utc();

    let object = stored(&backend, "d1").await.unwrap();
    assert!(object.starts_with(artifact));
    assert!(object.ends_with(MAGIC));
    let parsed = Trailer::parse_tail(&object, object.len() as u64).unwrap();
    let sealed_len = parsed.sealed_len();
    let trailer = parsed.trailer;
    let id = omni_remote_cache_service::hooks::api_key_id("k");
    assert_eq!(trailer.writer_credential.kind, "api_key");
    assert_eq!(trailer.writer_credential.id, id);
    assert_eq!(
        trailer.writer_principal, id,
        "the default principal is the credential id"
    );
    assert_eq!(trailer.namespace, "isolated");
    assert_eq!(trailer.scope.as_deref(), Some("default"));
    assert_eq!(trailer.signature, None);
    assert_eq!(trailer.size, artifact.len() as u64);
    assert_eq!(trailer.sha256, sha256_hex(artifact));
    assert!(trailer.stored_at >= before - time::Duration::seconds(1));
    assert!(trailer.stored_at <= after + time::Duration::seconds(1));
    assert_eq!(object.len(), artifact.len() + sealed_len);
    assert!(sealed_len > FOOTER_LEN);
}

#[tokio::test]
async fn nothing_the_client_sends_can_set_provenance() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    let artifact = b"forged";
    let status = authed(server.put(&path("d1")))
        .add_header("X-Omni-Content-Sha256", sha256_hex(artifact))
        .add_header("X-Omni-Principal", "someone-else")
        .add_header("X-Omni-Credential", "stolen")
        .add_header("X-Omni-Namespace", "isolated/other")
        .add_header("X-Omni-Scope", "pr-1")
        .add_header("X-Omni-Stored-At", "1999-01-01T00:00:00Z")
        .add_header("X-Omni-Signature", "key:AAAA")
        .add_header("X-Omni-Writer", "admin")
        .bytes(Bytes::from_static(artifact))
        .await
        .status_code();
    assert_eq!(status, StatusCode::NO_CONTENT);
    let object = stored(&backend, "d1").await.unwrap();
    let trailer = Trailer::parse_tail(&object, object.len() as u64)
        .unwrap()
        .trailer;
    let id = omni_remote_cache_service::hooks::api_key_id("k");
    assert_eq!(trailer.writer_principal, id);
    assert_eq!(trailer.writer_credential.id, id);
    assert_eq!(trailer.namespace, "isolated");
    assert_eq!(trailer.scope.as_deref(), Some("default"));
    assert_eq!(trailer.signature, None);
}

#[tokio::test]
async fn a_wrong_declared_hash_commits_nothing_and_leaves_the_key_free() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    let response = authed(server.put(&path("d1")))
        .add_header("X-Omni-Content-Sha256", sha256_hex(b"something else"))
        .bytes(Bytes::from_static(b"the real bytes"))
        .await;
    response.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
    assert!(stored(&backend, "d1").await.is_none());
    assert!(
        files(root.path()).is_empty(),
        "no staging file is left behind"
    );
    server
        .get(&path("d1"))
        .add_header("Authorization", "Bearer k")
        .add_header("X-OMNI-TENANT", "t")
        .await
        .assert_status(StatusCode::NOT_FOUND);

    // The key is still free, for the correct upload.
    assert_eq!(
        put(&server, "d1", b"the real bytes").await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_missing_or_malformed_hash_header_is_rejected_before_anything_is_read()
 {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    let good = sha256_hex(b"x");
    let upper = good.to_uppercase();
    let short = good[..63].to_string();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("missing", vec![]),
        ("uppercase", vec![&upper]),
        ("short", vec![&short]),
        ("empty", vec![""]),
        ("repeated", vec![&good, &good]),
    ];
    for (name, values) in cases {
        let mut request = authed(server.put(&path("d1")));
        for value in values {
            request = request.add_header("X-Omni-Content-Sha256", value);
        }
        request
            .bytes(Bytes::from_static(b"x"))
            .await
            .assert_status(StatusCode::BAD_REQUEST);
        assert!(stored(&backend, "d1").await.is_none(), "{name}");
    }
    assert!(files(root.path()).is_empty());
}

#[tokio::test]
async fn a_body_that_fails_part_way_commits_nothing() {
    use tower::ServiceExt as _;
    let root = tempfile::tempdir().unwrap();
    let (router, backend) = router(root.path()).await;
    let body = axum::body::Body::from_stream(tokio_stream::iter(vec![
        Ok::<_, std::io::Error>(Bytes::from_static(b"first half ")),
        Err(std::io::Error::other("connection reset")),
    ]));
    let request = http::Request::builder()
        .method("PUT")
        .uri(path("d1"))
        .header("Authorization", "Bearer k")
        .header("X-OMNI-TENANT", "t")
        .header(
            "X-Omni-Content-Sha256",
            sha256_hex(b"first half second half"),
        )
        .body(body)
        .unwrap();
    let status = router.oneshot(request).await.unwrap().status();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(stored(&backend, "d1").await.is_none());
    assert!(files(root.path()).is_empty());
}

#[tokio::test]
async fn get_returns_exactly_the_artifact_and_only_reader_safe_headers() {
    let root = tempfile::tempdir().unwrap();
    let (server, _) = server(root.path()).await;
    let artifact = b"exactly these bytes";
    put(&server, "d1", artifact).await;
    let response = authed(server.get(&path("d1"))).await;
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), artifact);
    assert_eq!(
        response.header("content-length"),
        artifact.len().to_string().as_str()
    );
    assert_eq!(
        response.header("x-omni-content-sha256"),
        sha256_hex(artifact).as_str()
    );
    assert_eq!(response.header("x-omni-namespace"), "isolated");
    time::OffsetDateTime::parse(
        response.header("x-omni-stored-at").to_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .expect("stored-at is RFC 3339");
    // Who wrote it is for operators, not for readers.
    for name in ["x-omni-credential", "x-omni-principal", "x-omni-signature"] {
        assert!(response.maybe_header(name).is_none(), "{name}");
    }
}

#[tokio::test]
async fn artifacts_that_look_like_a_trailer_or_are_empty_read_back_exactly() {
    let root = tempfile::tempdir().unwrap();
    let (server, _) = server(root.path()).await;
    let mut lookalike = b"payload".to_vec();
    lookalike.extend_from_slice(&1u32.to_be_bytes());
    lookalike.extend_from_slice(MAGIC);
    for (digest, artifact) in
        [("d1", lookalike.as_slice()), ("d2", b"".as_slice())]
    {
        assert_eq!(
            put(&server, digest, artifact).await,
            StatusCode::NO_CONTENT
        );
        let response = authed(server.get(&path(digest))).await;
        response.assert_status_ok();
        assert_eq!(response.as_bytes().as_ref(), artifact, "{digest}");
    }
}

#[tokio::test]
async fn a_large_artifact_beyond_the_tail_read_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let (server, _) = server(root.path()).await;
    let artifact: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    assert_eq!(put(&server, "d1", &artifact).await, StatusCode::NO_CONTENT);
    let response = authed(server.get(&path("d1"))).await;
    response.assert_status_ok();
    assert_eq!(response.as_bytes().as_ref(), artifact.as_slice());
}

#[tokio::test]
async fn head_agrees_with_get_for_stored_absent_and_invalid_objects() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    put(&server, "d1", b"stored").await;
    // Objects written without a trailer, as before this change.
    backend
        .save_if_absent(
            &container(),
            "prior1",
            Bytes::from_static(b"\x1f\x8b old archive bytes with no trailer"),
        )
        .await
        .unwrap();
    backend
        .save_if_absent(&container(), "tiny", Bytes::from_static(b"abc"))
        .await
        .unwrap();

    for (digest, get, head) in [
        ("d1", StatusCode::OK, StatusCode::NO_CONTENT),
        ("absent", StatusCode::NOT_FOUND, StatusCode::NOT_FOUND),
        ("prior1", StatusCode::NOT_FOUND, StatusCode::NOT_FOUND),
        ("tiny", StatusCode::NOT_FOUND, StatusCode::NOT_FOUND),
    ] {
        authed(server.get(&path(digest))).await.assert_status(get);
        authed(server.method(http::Method::HEAD, &path(digest)))
            .await
            .assert_status(head);
    }
}

#[tokio::test]
async fn a_tampered_stored_entry_is_a_miss_not_a_corrupt_hit() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    put(&server, "d1", b"original artifact").await;
    let object = stored(&backend, "d1").await.unwrap();

    for (name, tampered) in [
        ("truncated", object.slice(..object.len() - 5)),
        ("a byte flipped in the footer", {
            let mut bytes = object.to_vec();
            let at = bytes.len() - 3;
            bytes[at] ^= 0xff;
            Bytes::from(bytes)
        }),
        ("extra bytes appended", {
            let mut bytes = object.to_vec();
            bytes.extend_from_slice(b"more");
            Bytes::from(bytes)
        }),
        (
            "only the artifact",
            object.slice(.."original artifact".len()),
        ),
    ] {
        backend.delete(&container(), "d1").await.unwrap();
        backend
            .save_if_absent(&container(), "d1", tampered)
            .await
            .unwrap();
        let get = authed(server.get(&path("d1"))).await;
        assert_eq!(get.status_code(), StatusCode::NOT_FOUND, "GET, {name}");
        let head = authed(server.method(http::Method::HEAD, &path("d1"))).await;
        assert_eq!(head.status_code(), StatusCode::NOT_FOUND, "HEAD, {name}");
    }
}

/// A known limit, recorded as a test so it is not forgotten: until the client
/// verifies the hash (slice 7), bytes changed inside the artifact region of a
/// stored object are served, because the server checks the trailer and the
/// object's shape but does not re-hash the artifact on every read.
#[tokio::test]
async fn a_changed_byte_inside_the_artifact_is_not_detected_by_the_server_yet()
{
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    put(&server, "d1", b"original artifact").await;
    let mut object = stored(&backend, "d1").await.unwrap().to_vec();
    object[0] ^= 0xff;
    backend.delete(&container(), "d1").await.unwrap();
    backend
        .save_if_absent(&container(), "d1", Bytes::from(object))
        .await
        .unwrap();
    let response = authed(server.get(&path("d1"))).await;
    response.assert_status_ok();
    assert_ne!(response.as_bytes().as_ref(), b"original artifact");
    // The advertised hash is still the original one, which is what a verifying
    // client compares the bytes it received against.
    assert_eq!(
        response.header("x-omni-content-sha256"),
        sha256_hex(b"original artifact").as_str()
    );
}

#[tokio::test]
async fn a_conflicting_upload_leaves_the_first_entrys_provenance_intact() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    assert_eq!(put(&server, "d1", b"first").await, StatusCode::NO_CONTENT);
    let first = stored(&backend, "d1").await.unwrap();

    assert_eq!(
        put(&server, "d1", b"second, different").await,
        StatusCode::CONFLICT
    );
    assert_eq!(put(&server, "d1", b"first").await, StatusCode::CONFLICT);
    assert_eq!(
        stored(&backend, "d1").await.unwrap(),
        first,
        "byte for byte"
    );
    let response = authed(server.get(&path("d1"))).await;
    assert_eq!(response.as_bytes().as_ref(), b"first");
    assert!(
        files(root.path()).len() == 1,
        "no staging file from the rejected uploads"
    );
}

#[tokio::test]
async fn the_list_reports_the_stored_object_size_which_includes_the_trailer() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    let artifact = b"listed";
    put(&server, "d1", artifact).await;
    let object_len = stored(&backend, "d1").await.unwrap().len() as u64;
    assert!(object_len > artifact.len() as u64);

    let response =
        authed(server.get(&format!("/api/v1/artifacts?{QUERY}"))).await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    let item = &body["data"][0];
    assert_eq!(item["digest"], "d1");
    assert_eq!(item["size"], object_len);
}

#[tokio::test]
async fn the_size_cap_applies_to_artifact_bytes_not_to_the_sealed_object() {
    let root = tempfile::tempdir().unwrap();
    let args = ServeArgs::new(
        "".to_string(),
        Some(serde_json::to_string(&config()).unwrap()),
        Some(ConfigType::Inline),
        false,
        None,
        None,
        None,
        BackendType::InMemory,
        Some(RouterArgs::new(Some("/api".to_string()), false)),
        None,
    );
    let mut state = build_state(&args).await.unwrap();
    let backend = Arc::new(LocalDiskCacheBackend::new(root.path()));
    state.storage_backend = backend.clone();
    state.max_artifact_size = 8;
    let server = TestServer::new(
        omni_remote_cache_service::routes::root::build_router(
            &RouterConfig::from(args.routes.clone().unwrap()),
        )
        .with_state(state),
    );
    // Exactly at the cap is stored even though the object (with trailer) is larger.
    assert_eq!(
        put(&server, "d1", b"12345678").await,
        StatusCode::NO_CONTENT
    );
    assert!(stored(&backend, "d1").await.unwrap().len() > 8);
    assert_eq!(
        put(&server, "d2", b"123456789").await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(stored(&backend, "d2").await.is_none());
}

/// Storage that can serve a different object for a `GET` body than the one the
/// tail was read from, as a purge and re-publish between the two reads would.
struct Swappable {
    inner: LocalDiskCacheBackend,
    swap_in: std::sync::Mutex<Option<Bytes>>,
}

#[async_trait::async_trait]
impl RemoteCacheStorageBackend for Swappable {
    async fn get(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<Bytes>, Error> {
        self.inner.get(c, k).await
    }
    async fn exists(&self, c: &Container, k: &str) -> Result<bool, Error> {
        self.inner.exists(c, k).await
    }
    async fn get_stream(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<ReadStream>, Error> {
        if let Some(other) = self.swap_in.lock().unwrap().clone() {
            return Ok(Some(Box::pin(futures::stream::iter([Ok(other)]))));
        }
        self.inner.get_stream(c, k).await
    }
    async fn get_range(
        &self,
        c: &Container,
        k: &str,
        r: ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        self.inner.get_range(c, k, r).await
    }
    async fn list_prefix(
        &self,
        p: &ContainerPrefix,
        a: Option<&str>,
        l: u32,
    ) -> Result<PrefixPage, Error> {
        self.inner.list_prefix(p, a, l).await
    }
    async fn list_children(
        &self,
        p: &ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        self.inner.list_children(p).await
    }
    async fn delete_prefix(&self, p: &ContainerPrefix) -> Result<u64, Error> {
        self.inner.delete_prefix(p).await
    }
    async fn save_if_absent(
        &self,
        c: &Container,
        k: &str,
        b: Bytes,
    ) -> Result<SaveOutcome, Error> {
        self.inner.save_if_absent(c, k, b).await
    }
    async fn save_stream_if_absent(
        &self,
        c: &Container,
        k: &str,
        s: WriteStream,
    ) -> Result<SaveOutcome, Error> {
        self.inner.save_stream_if_absent(c, k, s).await
    }
    async fn delete(&self, c: &Container, k: &str) -> Result<(), Error> {
        self.inner.delete(c, k).await
    }
    async fn size(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<ByteSize>, Error> {
        self.inner.size(c, k).await
    }
}

/// A real HTTP server on a real socket, so the response goes through hyper
/// exactly as it does in production (an in-process router hands the body over
/// whole and cannot show what the transport does with it).
async fn serve(root: &std::path::Path) -> (String, Arc<Swappable>) {
    let args = ServeArgs::new(
        "".to_string(),
        Some(serde_json::to_string(&config()).unwrap()),
        Some(ConfigType::Inline),
        false,
        None,
        None,
        None,
        BackendType::InMemory,
        Some(RouterArgs::new(Some("/api".to_string()), false)),
        None,
    );
    let mut state = build_state(&args).await.unwrap();
    let backend = Arc::new(Swappable {
        inner: LocalDiskCacheBackend::new(root),
        swap_in: Default::default(),
    });
    state.storage_backend = backend.clone();
    let router = omni_remote_cache_service::routes::root::build_router(
        &RouterConfig::from(args.routes.clone().unwrap()),
    )
    .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (url, backend)
}

#[tokio::test]
async fn an_object_swapped_mid_read_fails_the_transfer_over_a_real_connection()
{
    let root = tempfile::tempdir().unwrap();
    let (url, backend) = serve(root.path()).await;
    let client = reqwest::Client::new();
    let artifact = b"the same artifact, published twice";
    let upload = |digest: &'static str| {
        let client = client.clone();
        let url = url.clone();
        async move {
            client
                .put(format!("{url}/api/v1/artifacts/{digest}?{QUERY}"))
                .bearer_auth("k")
                .header("X-OMNI-TENANT", "t")
                .header("X-Omni-Content-Sha256", sha256_hex(artifact))
                .body(artifact.to_vec())
                .send()
                .await
                .unwrap()
                .status()
        }
    };
    assert_eq!(upload("d1").await, StatusCode::NO_CONTENT);
    assert_eq!(upload("d2").await, StatusCode::NO_CONTENT);
    let get = || {
        client
            .get(format!("{url}/api/v1/artifacts/d1?{QUERY}"))
            .bearer_auth("k")
            .header("X-OMNI-TENANT", "t")
            .send()
    };

    // Control: the object as stored is delivered in full.
    let response = get().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap().as_ref(), artifact);

    // `d2` holds the same artifact with a different (later) trailer. Serving it
    // as the body of `d1` is what a purge and re-publish between the reads does.
    let other = backend
        .inner
        .get(&container(), "d2")
        .await
        .unwrap()
        .unwrap();
    *backend.swap_in.lock().unwrap() = Some(other);
    // The client must never receive a successful body that is paired with the
    // wrong provenance. Depending on when hyper flushes the headers the transfer
    // fails either at the request or while reading the body.
    match get().await {
        Err(_) => {}
        Ok(response) => assert!(
            response.bytes().await.is_err(),
            "a swapped object must not be delivered as a successful body"
        ),
    }
}
