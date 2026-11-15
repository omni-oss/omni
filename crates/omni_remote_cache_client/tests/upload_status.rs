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
