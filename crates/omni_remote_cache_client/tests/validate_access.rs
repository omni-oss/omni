use std::time::Duration;

use omni_remote_cache_client::{
    DefaultRemoteCacheClient, RemoteAccessArgs, RemoteCacheClient,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

/// Answers one request with `response` and returns the request head, so tests
/// can check what the client sent.
async fn probe(
    response: &'static str,
) -> (omni_remote_cache_client::ValidateAccessResult, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        timeout(Duration::from_secs(15), async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(socket.read_u8().await.unwrap());
                assert!(head.len() < 32 * 1024);
            }
            socket.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8(head).unwrap()
        })
        .await
        .unwrap()
    });
    let access = RemoteAccessArgs::new(&url, "secret-key", "t", "o", "w", "e");
    let result = timeout(
        Duration::from_secs(15),
        DefaultRemoteCacheClient::default().validate_access(&access),
    )
    .await
    .unwrap()
    .unwrap();
    (result, server.await.unwrap())
}

#[tokio::test]
async fn a_valid_probe_reports_the_scopes_and_size_cap_the_server_advertises() {
    let (result, head) = probe(
        "HTTP/1.1 204 No Content\r\nx-omni-scopes: read:artifacts, write:artifacts,,\r\nx-omni-max-artifact-size: 33554432\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(result.is_valid);
    assert_eq!(result.scopes, ["read:artifacts", "write:artifacts"]);
    assert_eq!(result.max_artifact_size, Some(33_554_432));
    assert!(head.starts_with("HEAD /v1/artifacts?"), "{head}");
    let lower = head.to_ascii_lowercase();
    assert!(lower.contains("authorization: bearer secret-key"), "{head}");
    assert!(!lower.contains("x-api-key"), "{head}");
}

#[tokio::test]
async fn missing_or_malformed_advertisements_are_not_invented() {
    for response in [
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 204 No Content\r\nx-omni-max-artifact-size: lots\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 204 No Content\r\nx-omni-max-artifact-size: -1\r\nConnection: close\r\n\r\n",
    ] {
        let (result, _) = probe(response).await;
        assert!(result.is_valid);
        assert!(result.scopes.is_empty());
        assert_eq!(result.max_artifact_size, None, "{response}");
    }
}

#[tokio::test]
async fn a_rejected_probe_ignores_advertised_values() {
    for status in ["401 Unauthorized", "403 Forbidden"] {
        let response = Box::leak(
            format!(
                "HTTP/1.1 {status}\r\nx-omni-scopes: write:artifacts\r\nx-omni-max-artifact-size: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .into_boxed_str(),
        );
        let (result, _) = probe(response).await;
        assert!(!result.is_valid, "{status}");
        assert!(result.scopes.is_empty(), "{status}");
        assert_eq!(result.max_artifact_size, None, "{status}");
    }
}
