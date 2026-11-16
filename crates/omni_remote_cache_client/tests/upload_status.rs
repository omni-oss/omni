use std::time::Duration;

use bytes::Bytes;
use omni_remote_cache_client::{
    DefaultRemoteCacheClient, RemoteAccessArgs, RemoteCacheClient,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::timeout,
};

#[tokio::test]
async fn uploads_accept_conflicts_but_keep_other_failures_visible_to_callers() {
    for (status, success) in
        [(204, true), (409, true), (403, false), (503, false)]
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            timeout(Duration::from_secs(15), async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                    assert!(header.len() < 32 * 1024);
                }
                let header = String::from_utf8(header).unwrap();
                assert!(header.starts_with("PUT /v1/artifacts/d?"));
                let length = header.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                }).unwrap();
                assert_eq!(length, 7);
                // The server seals the entry only if the bytes match this.
                let declared: Vec<_> = header
                    .lines()
                    .filter_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("x-omni-content-sha256")
                            .then(|| value.trim().to_owned())
                    })
                    .collect();
                assert_eq!(declared, [sha256_hex(b"archive")]);
                let mut body = [0; 7];
                socket.read_exact(&mut body).await.unwrap();
                assert_eq!(&body, b"archive");
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                socket.write_all(response.as_bytes()).await.unwrap();
            }).await.unwrap();
        });
        let client = DefaultRemoteCacheClient::default();
        let access = RemoteAccessArgs::new(&url, "test", "t", "o", "w", "e");
        let result = timeout(
            Duration::from_secs(15),
            client.put_artifact(&access, "d", Bytes::from_static(b"archive")),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(result.is_ok(), success, "upload status {status}");
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Serves one `PUT` and returns the declared hash and the body it received.
async fn upload(artifact: Bytes) -> (Vec<String>, Vec<u8>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let expected = artifact.len();
    let server = tokio::spawn(async move {
        timeout(Duration::from_secs(30), async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(socket.read_u8().await.unwrap());
            }
            let head = String::from_utf8(head).unwrap();
            let declared = head
                .lines()
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("x-omni-content-sha256")
                        .then(|| value.trim().to_owned())
                })
                .collect::<Vec<_>>();
            let mut body = vec![0; expected];
            socket.read_exact(&mut body).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            (declared, body)
        })
        .await
        .unwrap()
    });
    let access = RemoteAccessArgs::new(&url, "k", "t", "o", "w", "e");
    DefaultRemoteCacheClient::default()
        .put_artifact(&access, "d", artifact)
        .await
        .unwrap();
    server.await.unwrap()
}

#[tokio::test]
async fn the_declared_hash_matches_the_bytes_sent_for_any_size() {
    let large: Vec<u8> = (0..6_000_000u32).map(|i| (i * 31) as u8).collect();
    for artifact in [Bytes::new(), Bytes::from_static(b"x"), Bytes::from(large)]
    {
        let (declared, body) = upload(artifact.clone()).await;
        assert_eq!(body, artifact.to_vec());
        assert_eq!(
            declared,
            [sha256_hex(&artifact)],
            "{} bytes",
            artifact.len()
        );
    }
}
