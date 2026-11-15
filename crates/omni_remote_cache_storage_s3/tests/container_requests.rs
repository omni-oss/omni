use std::time::Duration;

use aws_config::{BehaviorVersion, Region, retry::RetryConfig};
use aws_credential_types::Credentials;
use bytes::Bytes;
use omni_remote_cache_storage_core::{
    Container, PageOptions, RemoteCacheStorageBackend, SaveOutcome,
};
use omni_remote_cache_storage_s3::S3CacheBackend;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::timeout,
};

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        409 => "Conflict",
        412 => "Precondition Failed",
        500 => "Internal Server Error",
        other => panic!("no scripted reason phrase for status {other}"),
    }
}

// Exercise the real SDK's request serialization and response decoding,
// without requiring an external bucket or credentials. Each scripted reply is
// (status, content type, body).
async fn backend(
    replies: Vec<(u16, &'static str, &'static str)>,
) -> (S3CacheBackend, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let requests = tokio::spawn(async move {
        timeout(Duration::from_secs(15), async move {
            let mut requests = Vec::new();
            for (status, content_type, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                    assert!(header.len() < 32 * 1024);
                }
                let header = String::from_utf8(header).unwrap();
                let length = header.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                }).unwrap_or(0);
                assert!(length < 1024 * 1024);
                let mut upload = vec![0; length];
                socket.read_exact(&mut upload).await.unwrap();
                requests.push(format!("{header}{}", String::from_utf8_lossy(&upload)));
                let response = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    reason_phrase(status),
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        })
        .await
        .expect("SDK did not issue the expected requests")
    });
    (backend_at(endpoint), requests)
}

fn backend_at(endpoint: String) -> S3CacheBackend {
    let config = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(Credentials::new(
            "test", "test", None, None, "test",
        ))
        .endpoint_url(endpoint)
        .force_path_style(true)
        // The SDK retries 5xx on its own; disable that so request counts
        // reflect only the backend's own retry policy.
        .retry_config(RetryConfig::disabled())
        .build();
    S3CacheBackend::from_aws_s3_sdk_config(config, "cache")
}

const LIST: &str = r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><IsTruncated>false</IsTruncated><Contents><Key>ns/ci/</Key><Size>0</Size></Contents><Contents><Key>ns/ci/d</Key><Size>7</Size></Contents></ListBucketResult>"#;

async fn listing_round_trip(paged: bool) {
    let (backend, requests) = backend(vec![
        (200, "application/xml", LIST),
        (200, "application/octet-stream", "archive"),
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
    let (backend, requests) = backend(vec![(200, "application/xml", "")]).await;
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

fn assert_create_only(request: &str) {
    let header = request.split_once("\r\n\r\n").unwrap().0;
    let if_none_match = header.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("if-none-match")
            .then(|| value.trim())
    });
    assert_eq!(if_none_match, Some("*"));
}

// Returns the payload the server received. Streamed uploads are framed as
// aws-chunked (`<hex size>;chunk-signature=..` lines), so decode those.
fn request_body(request: &str) -> String {
    let (header, body) = request.split_once("\r\n\r\n").unwrap();
    if !header.to_ascii_lowercase().contains("aws-chunked") {
        return body.to_string();
    }
    let mut payload = String::new();
    let mut rest = body;
    loop {
        let (line, after) = rest.split_once("\r\n").unwrap();
        let size =
            usize::from_str_radix(line.split(';').next().unwrap(), 16).unwrap();
        if size == 0 {
            break;
        }
        payload.push_str(&after[..size]);
        rest = &after[size + 2..];
    }
    payload
}

#[tokio::test]
async fn streamed_upload_flushes_the_staged_file_before_put() {
    let (backend, requests) = backend(vec![(200, "application/xml", "")]).await;
    let container = Container::parse("ns/ci").unwrap();
    let stream = tokio_stream::iter([
        Ok(Bytes::from_static(b"arch")),
        Ok(Bytes::from_static(b"ive")),
    ]);
    backend
        .save_stream_if_absent(&container, "d", Box::pin(stream))
        .await
        .unwrap();
    let requests = requests.await.unwrap();
    assert_request_path(&requests[0], "PUT", "/cache/ns/ci/d");
    assert_create_only(&requests[0]);
    assert!(
        requests[0]
            .split_once("\r\n\r\n")
            .unwrap()
            .1
            .contains("archive")
    );
}

#[tokio::test]
async fn save_if_absent_sends_create_only_put() {
    let (backend, requests) = backend(vec![(200, "application/xml", "")]).await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .save_if_absent(&container, "d", Bytes::from_static(b"archive"))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request_path(&requests[0], "PUT", "/cache/ns/ci/d");
    assert_create_only(&requests[0]);
}

#[tokio::test]
async fn failed_upload_stream_never_reaches_s3() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let backend =
        backend_at(format!("http://{}", listener.local_addr().unwrap()));
    let (stop, mut stopped) = tokio::sync::oneshot::channel();
    let requests = tokio::spawn(async move {
        let mut observed = Vec::new();
        loop {
            tokio::select! {
                // Drain queued connections before honoring stop, so an
                // attempted request cannot hide behind the shutdown signal.
                biased;
                accepted = listener.accept() => {
                    let (mut socket, _) = accepted.unwrap();
                    let mut header = Vec::new();
                    timeout(Duration::from_secs(5), async {
                        while !header.ends_with(b"\r\n\r\n") {
                            header.push(socket.read_u8().await.unwrap());
                            assert!(header.len() < 32 * 1024);
                        }
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                    }).await.unwrap();
                    observed.push(header);
                }
                _ = &mut stopped => break,
            }
        }
        observed
    });
    let container = Container::parse("ns/ci").unwrap();
    let stream = tokio_stream::iter([
        Ok(Bytes::from_static(b"partial")),
        Err(omni_remote_cache_storage_core::error::Error::custom(
            std::io::Error::other("interrupted upload"),
        )),
    ]);
    let error = backend
        .save_stream_if_absent(&container, "d", Box::pin(stream))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("interrupted upload"));
    stop.send(()).unwrap();
    assert!(requests.await.unwrap().is_empty());
}

#[tokio::test]
async fn precondition_failed_put_reports_already_exists() {
    let (backend, requests) = backend(vec![(412, "application/xml", "")]).await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .save_if_absent(&container, "d", Bytes::from_static(b"archive"))
            .await
            .unwrap(),
        SaveOutcome::AlreadyExists
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request_path(&requests[0], "PUT", "/cache/ns/ci/d");
    assert_create_only(&requests[0]);
}

#[tokio::test]
async fn precondition_failed_stream_put_reports_already_exists() {
    let (backend, requests) = backend(vec![(412, "application/xml", "")]).await;
    let container = Container::parse("ns/ci").unwrap();
    let stream = tokio_stream::iter([Ok(Bytes::from_static(b"archive"))]);
    assert_eq!(
        backend
            .save_stream_if_absent(&container, "d", Box::pin(stream))
            .await
            .unwrap(),
        SaveOutcome::AlreadyExists
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request_path(&requests[0], "PUT", "/cache/ns/ci/d");
    assert_create_only(&requests[0]);
}

#[tokio::test]
async fn conflict_then_success_retries_with_full_body() {
    let (backend, requests) = backend(vec![
        (409, "application/xml", ""),
        (200, "application/xml", ""),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .save_if_absent(&container, "d", Bytes::from_static(b"archive"))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_request_path(request, "PUT", "/cache/ns/ci/d");
        assert_create_only(request);
        assert_eq!(request_body(request), "archive");
    }
}

#[tokio::test]
async fn stream_conflict_then_success_rebuilds_body_per_attempt() {
    let (backend, requests) = backend(vec![
        (409, "application/xml", ""),
        (200, "application/xml", ""),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    let stream = tokio_stream::iter([
        Ok(Bytes::from_static(b"arch")),
        Ok(Bytes::from_static(b"ive")),
    ]);
    assert_eq!(
        backend
            .save_stream_if_absent(&container, "d", Box::pin(stream))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_request_path(request, "PUT", "/cache/ns/ci/d");
        assert_create_only(request);
        assert_eq!(request_body(request), "archive");
    }
}

#[tokio::test]
async fn persistent_conflict_gives_up_after_four_attempts() {
    let (backend, requests) =
        backend(vec![(409, "application/xml", ""); 4]).await;
    let container = Container::parse("ns/ci").unwrap();
    backend
        .save_if_absent(&container, "d", Bytes::from_static(b"archive"))
        .await
        .unwrap_err();
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 4);
    for request in &requests {
        assert_request_path(request, "PUT", "/cache/ns/ci/d");
        assert_eq!(request_body(request), "archive");
    }
}

#[tokio::test]
async fn stream_persistent_conflict_gives_up_after_four_attempts() {
    let (backend, requests) =
        backend(vec![(409, "application/xml", ""); 4]).await;
    let container = Container::parse("ns/ci").unwrap();
    let stream = tokio_stream::iter([Ok(Bytes::from_static(b"archive"))]);
    backend
        .save_stream_if_absent(&container, "d", Box::pin(stream))
        .await
        .unwrap_err();
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 4);
    for request in &requests {
        assert_request_path(request, "PUT", "/cache/ns/ci/d");
        assert_eq!(request_body(request), "archive");
    }
}

#[tokio::test]
async fn server_error_is_not_retried() {
    let (backend, requests) = backend(vec![(500, "application/xml", "")]).await;
    let container = Container::parse("ns/ci").unwrap();
    backend
        .save_if_absent(&container, "d", Bytes::from_static(b"archive"))
        .await
        .unwrap_err();
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request_path(&requests[0], "PUT", "/cache/ns/ci/d");
}
