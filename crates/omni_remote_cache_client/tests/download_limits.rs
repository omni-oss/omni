use std::time::Duration;

use omni_remote_cache_client::{
    DefaultRemoteCacheClient, Placement, RemoteAccessArgs, RemoteCacheClient,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

/// Serves `response` bytes to one request and returns the request head.
async fn serve(
    response: Vec<u8>,
    cap: Option<u64>,
    placement: Placement,
    scope: Option<&'static str>,
) -> (Option<bytes::Bytes>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        timeout(Duration::from_secs(15), async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(socket.read_u8().await.unwrap());
            }
            let _ = socket.write_all(&response).await;
            let _ = socket.shutdown().await;
            String::from_utf8(head).unwrap()
        })
        .await
        .unwrap()
    });
    let mut access = RemoteAccessArgs::new(&url, "k", "t", "o", "w", "e");
    access.placement = placement;
    access.scope = scope;
    access.max_artifact_size = cap;
    let got = timeout(
        Duration::from_secs(15),
        DefaultRemoteCacheClient::default().get_artifact(&access, "d1"),
    )
    .await
    .unwrap()
    .unwrap();
    (got, server.await.unwrap())
}

fn ok(body: &[u8], declared: usize) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

#[tokio::test]
async fn a_body_within_the_cap_is_returned() {
    let (got, _) =
        serve(ok(b"hello", 5), Some(5), Placement::Isolated, None).await;
    assert_eq!(got.as_deref(), Some(&b"hello"[..]));
}

#[tokio::test]
async fn a_declared_length_over_the_cap_is_a_miss() {
    let (got, _) =
        serve(ok(b"hello", 5), Some(4), Placement::Isolated, None).await;
    assert_eq!(got, None);
}

#[tokio::test]
async fn a_body_that_lies_about_a_small_length_is_cut_off_by_the_transport() {
    // Claims 2 bytes and sends 5: the client never reads more than declared.
    let (got, _) =
        serve(ok(b"hello", 2), Some(4), Placement::Isolated, None).await;
    assert_eq!(got.as_deref(), Some(&b"he"[..]));
}

#[tokio::test]
async fn a_chunked_body_over_the_cap_is_a_miss() {
    let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n".to_vec();
    let (got, _) = serve(response, Some(5), Placement::Isolated, None).await;
    assert_eq!(got, None);
}

#[tokio::test]
async fn a_truncated_body_is_a_miss_not_an_error() {
    let (got, _) =
        serve(ok(b"abc", 10), Some(100), Placement::Isolated, None).await;
    assert_eq!(got, None);
}

#[tokio::test]
async fn without_a_configured_cap_the_default_applies() {
    let (got, _) = serve(ok(b"x", 1), None, Placement::Isolated, None).await;
    assert_eq!(got.as_deref(), Some(&b"x"[..]));
}

#[tokio::test]
async fn the_scope_is_sent_only_for_isolated_requests_and_only_when_valid() {
    let (_, head) =
        serve(ok(b"x", 1), None, Placement::Isolated, Some("pr-4")).await;
    assert!(head.contains("&namespace=isolated&scope=pr-4"), "{head}");
    let (_, head) =
        serve(ok(b"x", 1), None, Placement::Shared, Some("pr-4")).await;
    assert!(!head.contains("scope"), "{head}");
    let (_, head) =
        serve(ok(b"x", 1), None, Placement::Isolated, Some("../x")).await;
    assert!(!head.contains("scope"), "{head}");
}

async fn entry(head_extra: &str) -> omni_remote_cache_client::RemoteArtifact {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n{head_extra}Connection: close\r\n\r\nhi"
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(socket.read_u8().await.unwrap());
        }
        let _ = socket.write_all(response.as_bytes()).await;
    });
    let access = RemoteAccessArgs::new(&url, "k", "t", "o", "w", "e");
    let got = DefaultRemoteCacheClient::default()
        .get_artifact_entry(&access, "d1")
        .await
        .unwrap()
        .unwrap();
    server.abort();
    got
}

#[tokio::test]
async fn provenance_headers_are_read_once_and_duplicates_are_ambiguous() {
    let one =
        entry("x-omni-content-sha256: abc\r\nx-omni-signature: sig\r\n").await;
    assert_eq!(one.sha256.as_deref(), Some("abc"));
    assert_eq!(one.signature.as_deref(), Some("sig"));
    assert!(!one.ambiguous_provenance);
    let none = entry("").await;
    assert_eq!((none.sha256, none.signature), (None, None));
    assert!(!none.ambiguous_provenance);
    let twice = entry("x-omni-signature: a\r\nx-omni-signature: b\r\n").await;
    assert!(twice.ambiguous_provenance);
    assert_eq!(twice.signature, None);
    let twice =
        entry("x-omni-content-sha256: a\r\nx-omni-content-sha256: b\r\n").await;
    assert!(twice.ambiguous_provenance);
}

#[tokio::test]
async fn a_signature_is_sent_only_on_shared_uploads() {
    for (placement, expect) in
        [(Placement::Shared, true), (Placement::Isolated, false)]
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(socket.read_u8().await.unwrap());
            }
            let text = String::from_utf8(head).unwrap();
            let length: usize = text
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            let _ = socket.read_exact(&mut body).await;
            let _ = socket
                .write_all(b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await;
            text
        });
        let mut access = RemoteAccessArgs::new(&url, "k", "t", "o", "w", "e");
        access.placement = placement;
        access.signature = Some("sig-value");
        let outcome = DefaultRemoteCacheClient::default()
            .put_artifact_outcome(
                &access,
                "d1",
                bytes::Bytes::from_static(b"x"),
            )
            .await
            .unwrap();
        assert_eq!(
            outcome,
            omni_remote_cache_client::PutOutcome::AlreadyExists
        );
        let head = server.await.unwrap().to_ascii_lowercase();
        assert_eq!(
            head.contains("x-omni-signature: sig-value"),
            expect,
            "{placement:?}"
        );
    }
}
