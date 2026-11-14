use std::time::Duration;

use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use bytes::Bytes;
use omni_remote_cache_storage_core::{
    Container, PageOptions, RemoteCacheStorageBackend,
};
use omni_remote_cache_storage_s3::S3CacheBackend;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

// Exercise the real SDK's request serialization and response decoding,
// without requiring an external bucket or credentials.
async fn backend(
    replies: Vec<(&'static str, &'static str)>,
) -> (S3CacheBackend, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let requests = tokio::spawn(async move {
        timeout(Duration::from_secs(15), async move {
            let mut requests = Vec::new();
            for (content_type, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                    assert!(header.len() < 32 * 1024);
                }
                requests.push(String::from_utf8(header).unwrap());
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        })
        .await
        .expect("SDK did not issue the expected requests")
    });
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new(
            "test", "test", None, None, "test",
        ))
        .endpoint_url(endpoint)
        .force_path_style(true)
        .build();
    (
        S3CacheBackend::from_aws_s3_sdk_config(config, "cache"),
        requests,
    )
}

const LIST: &str = r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><IsTruncated>false</IsTruncated><Contents><Key>ns/ci/</Key><Size>0</Size></Contents><Contents><Key>ns/ci/d</Key><Size>7</Size></Contents></ListBucketResult>"#;

async fn listing_round_trip(paged: bool) {
    let (backend, requests) = backend(vec![
        ("application/xml", LIST),
        ("application/octet-stream", "archive"),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    let items = if paged {
        backend
            .paged_list(&container, PageOptions::new(None, Some(10)))
            .await
            .unwrap()
    } else {
        backend.list(&container).await.unwrap()
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].key, "d");
    assert_eq!(items[0].size.as_u64(), 7);
    assert_eq!(
        backend.get(&container, &items[0].key).await.unwrap(),
        Some(Bytes::from_static(b"archive"))
    );
    let requests = requests.await.unwrap();
    assert!(
        requests[0]
            .lines()
            .next()
            .unwrap()
            .contains("prefix=ns%2Fci%2F")
    );
    assert_request_path(&requests[1], "GET", "/cache/ns/ci/d");
}

#[tokio::test]
async fn list_returns_retrievable_container_relative_keys() {
    listing_round_trip(false).await;
}

#[tokio::test]
async fn paged_list_returns_retrievable_container_relative_keys() {
    listing_round_trip(true).await;
}

#[tokio::test]
async fn delete_targets_the_container_not_the_bucket_root() {
    let (backend, requests) = backend(vec![("application/xml", "")]).await;
    let container = Container::parse("ns/ci").unwrap();
    backend.delete(&container, "d").await.unwrap();
    let requests = requests.await.unwrap();
    assert_request_path(&requests[0], "DELETE", "/cache/ns/ci/d");
}

fn assert_request_path(request: &str, method: &str, path: &str) {
    let mut parts = request.lines().next().unwrap().split_whitespace();
    assert_eq!(parts.next(), Some(method));
    // The SDK also supplies an operation identifier in the query string.
    assert_eq!(parts.next().unwrap().split('?').next(), Some(path));
}
