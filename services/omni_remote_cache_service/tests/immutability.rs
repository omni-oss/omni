use std::sync::Arc;

use axum_test::TestServer;
use bytes::Bytes;
use http::StatusCode;
use maps::unordered_map;
use omni_remote_cache_service::{
    args::{BackendType, ConfigType, RouterArgs, ServeArgs},
    config::{
        All, AllOrSpecificConfiguration, ApiKeyConfiguration, Configuration,
        EnvironmentConfiguration, OrganizationConfiguration,
        SecurityConfiguration, TenantConfiguration, WorkspaceConfiguration,
    },
    routes::root::RouterConfig,
    state::build_state,
};
use omni_remote_cache_storage_core::{Container, RemoteCacheStorageBackend};
use omni_remote_cache_storage_local::LocalDiskCacheBackend;

const DIGEST: &str = "6vQEbzmN9ofk2uWkvxhq8PRPmdZLVPUSw1YJ3tUNyrSd";
const QUERY: &str = "org=o&ws=w&env=e";

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
            api_keys: unordered_map!("k".to_string() => ApiKeyConfiguration {
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

/// A server whose storage is a real local-disk backend in `root`, which is
/// also handed back so tests can inspect it directly.
async fn server(
    root: &std::path::Path,
) -> (TestServer, Arc<LocalDiskCacheBackend>) {
    let (router, backend) = router(root, None).await;
    (TestServer::new(router), backend)
}

/// The router itself, optionally with a smaller `max_artifact_size`.
async fn router(
    root: &std::path::Path,
    max_artifact_size: Option<u64>,
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
    if let Some(limit) = max_artifact_size {
        state.max_artifact_size = limit;
    }
    let router = omni_remote_cache_service::routes::root::build_router(
        &RouterConfig::from(args.routes.clone().unwrap()),
    )
    .with_state(state);
    (router, backend)
}

fn container() -> Container {
    Container::parse("t/o/w/e").unwrap()
}

async fn put(
    server: &TestServer,
    digest: &str,
    body: &'static str,
) -> StatusCode {
    server
        .put(&format!("/api/v1/artifacts/{digest}?{QUERY}"))
        .add_header("X-API-KEY", "k")
        .add_header("X-OMNI-TENANT", "t")
        .bytes(Bytes::from(body))
        .await
        .status_code()
}

#[tokio::test]
async fn a_second_put_conflicts_and_never_replaces_the_stored_entry() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;

    assert_eq!(put(&server, DIGEST, "first").await, StatusCode::NO_CONTENT);
    assert_eq!(put(&server, DIGEST, "second").await, StatusCode::CONFLICT);
    assert_eq!(put(&server, DIGEST, "first").await, StatusCode::CONFLICT);

    assert_eq!(
        backend.get(&container(), DIGEST).await.unwrap(),
        Some(Bytes::from("first")),
        "the first writer's bytes must survive"
    );
}

#[tokio::test]
async fn concurrent_first_writers_agree_on_exactly_one_winner() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;
    let bodies: [&'static str; 8] = [
        "writer-0", "writer-1", "writer-2", "writer-3", "writer-4", "writer-5",
        "writer-6", "writer-7",
    ];

    let statuses = futures::future::join_all(
        bodies.iter().map(|body| put(&server, DIGEST, body)),
    )
    .await;

    let winners: Vec<usize> = statuses
        .iter()
        .enumerate()
        .filter(|(_, s)| **s == StatusCode::NO_CONTENT)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(winners.len(), 1, "exactly one 204, got {statuses:?}");
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        bodies.len() - 1,
        "every loser gets 409, got {statuses:?}"
    );
    assert_eq!(
        backend.get(&container(), DIGEST).await.unwrap(),
        Some(Bytes::from(bodies[winners[0]])),
        "the stored bytes belong to the winner, not a blend"
    );
}

#[tokio::test]
async fn malformed_digests_are_rejected_before_storage_is_touched() {
    let root = tempfile::tempdir().unwrap();
    let (server, backend) = server(root.path()).await;

    // A bare `..` is normalized away by URL handling before it can reach the
    // route, so it is covered by the percent-encoded traversal form below.
    for bad in [
        "..%2F..%2Fshared%2FX",
        "a.b",
        "a-b",
        "0abc",
        "has%20space",
        &"a".repeat(65),
    ] {
        assert_eq!(
            put(&server, bad, "evil").await,
            StatusCode::BAD_REQUEST,
            "{bad:?} must be rejected"
        );
    }

    assert!(
        backend.list(&container()).await.unwrap().is_empty(),
        "a rejected digest must not create anything"
    );
    let mut entries = std::fs::read_dir(root.path()).unwrap();
    assert!(
        entries.all(|e| e.unwrap().file_type().unwrap().is_dir()),
        "no stray files under the storage root"
    );
}

#[tokio::test]
async fn authentication_comes_before_digest_validation() {
    let root = tempfile::tempdir().unwrap();
    let (server, _backend) = server(root.path()).await;
    let response = server
        .put(&format!("/api/v1/artifacts/not.a.digest?{QUERY}"))
        .add_header("X-API-KEY", "wrong-key")
        .add_header("X-OMNI-TENANT", "t")
        .bytes(Bytes::from("x"))
        .await;
    assert_ne!(
        response.status_code(),
        StatusCode::BAD_REQUEST,
        "an unauthenticated caller must not learn anything about digest validity"
    );
    assert!(
        response.status_code() == StatusCode::UNAUTHORIZED
            || response.status_code() == StatusCode::FORBIDDEN,
        "got {}",
        response.status_code()
    );
}

fn regular_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
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

async fn put_stream(
    router: &axum::Router,
    digest: &str,
    chunks: Vec<&'static str>,
) -> StatusCode {
    use tower::ServiceExt as _;
    // A streamed body has no Content-Length, so only the stream bound can stop it.
    let body = axum::body::Body::from_stream(tokio_stream::iter(
        chunks
            .into_iter()
            .map(|c| Ok::<_, std::io::Error>(Bytes::from(c))),
    ));
    let request = http::Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/artifacts/{digest}?{QUERY}"))
        .header("X-API-KEY", "k")
        .header("X-OMNI-TENANT", "t")
        .body(body)
        .unwrap();
    router.clone().oneshot(request).await.unwrap().status()
}

/// A PUT that declares its length, as a real client does. The in-process
/// `TestServer` sends no Content-Length, so build the request by hand.
async fn put_declared(
    router: &axum::Router,
    digest: &str,
    body: &'static str,
) -> StatusCode {
    use tower::ServiceExt as _;
    let request = http::Request::builder()
        .method("PUT")
        .uri(format!("/api/v1/artifacts/{digest}?{QUERY}"))
        .header("X-API-KEY", "k")
        .header("X-OMNI-TENANT", "t")
        .header(http::header::CONTENT_LENGTH, body.len().to_string())
        .body(axum::body::Body::from(body))
        .unwrap();
    router.clone().oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn an_upload_over_the_limit_is_rejected_by_content_length() {
    let root = tempfile::tempdir().unwrap();
    let (router, backend) = router(root.path(), Some(8)).await;

    assert_eq!(
        put_declared(&router, DIGEST, "123456789").await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(backend.get(&container(), DIGEST).await.unwrap().is_none());
    assert!(regular_files(root.path()).is_empty(), "nothing is staged");

    // Exactly at the limit is accepted, declared or streamed.
    assert_eq!(
        put_declared(&router, DIGEST, "12345678").await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_streamed_upload_over_the_limit_commits_nothing() {
    let root = tempfile::tempdir().unwrap();
    let (router, backend) = router(root.path(), Some(8)).await;

    assert_eq!(
        put_stream(&router, DIGEST, vec!["1234", "5678", "9"]).await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(
        backend.get(&container(), DIGEST).await.unwrap().is_none(),
        "the key must stay free after an oversized stream"
    );
    assert!(
        regular_files(root.path()).is_empty(),
        "staging is cleaned up"
    );

    // The key is still usable afterwards, and at the limit it is accepted.
    assert_eq!(
        put_stream(&router, DIGEST, vec!["1234", "5678"]).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        backend.get(&container(), DIGEST).await.unwrap(),
        Some(Bytes::from("12345678"))
    );
}

#[tokio::test]
async fn an_oversized_upload_never_disturbs_an_existing_entry() {
    let root = tempfile::tempdir().unwrap();
    let (router, backend) = router(root.path(), Some(8)).await;
    let server = TestServer::new(router.clone());

    assert_eq!(
        put(&server, DIGEST, "original").await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        put_stream(&router, DIGEST, vec!["too", "much", "data"]).await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        backend.get(&container(), DIGEST).await.unwrap(),
        Some(Bytes::from("original"))
    );
}
