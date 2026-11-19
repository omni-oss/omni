use std::time::Duration;

use aws_config::{BehaviorVersion, Region, retry::RetryConfig};
use aws_credential_types::Credentials;
use bytes::Bytes;
use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, RemoteCacheStorageBackend,
    SaveOutcome,
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
        404 => "Not Found",
        409 => "Conflict",
        412 => "Precondition Failed",
        416 => "Range Not Satisfiable",
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

#[tokio::test]
async fn list_returns_retrievable_container_relative_keys() {
    let (backend, requests) = backend(vec![
        (200, "application/xml", LIST),
        (200, "application/octet-stream", "archive"),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    let items = backend.list(&container).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].key, "d");
    assert_eq!(items[0].size.as_u64(), 7);
    assert_eq!(
        backend.get(&container, &items[0].key).await.unwrap(),
        Some(Bytes::from_static(b"archive"))
    );
    let requests = requests.await.unwrap();
    assert_request_path(&requests[0], "GET", "/cache/");
    assert!(request_line(&requests[0]).contains("prefix=ns%2Fci%2F"));
    assert_request_path(&requests[1], "GET", "/cache/ns/ci/d");
}

const PAGE_TRUNCATED: &str = r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><MaxKeys>10</MaxKeys><IsTruncated>true</IsTruncated><NextContinuationToken>tok1</NextContinuationToken><Contents><Key>ns/ci/</Key><Size>0</Size></Contents><Contents><Key>ns/ci/a/b</Key><Size>3</Size></Contents><Contents><Key>ns/ci/c</Key><Size>4</Size></Contents></ListBucketResult>"#;

const PAGE_LAST: &str = r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><MaxKeys>10</MaxKeys><IsTruncated>false</IsTruncated><Contents><Key>ns/ci/d</Key><Size>5</Size></Contents></ListBucketResult>"#;

const PAGE_MARKER_ONLY: &str = r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><MaxKeys>1</MaxKeys><IsTruncated>true</IsTruncated><Contents><Key>ns/ci/</Key><Size>0</Size></Contents></ListBucketResult>"#;

fn request_line(request: &str) -> &str {
    request.lines().next().unwrap()
}

#[tokio::test]
async fn list_prefix_sends_prefix_limit_and_cursor_and_returns_relative_keys() {
    let (backend, requests) =
        backend(vec![(200, "application/xml", PAGE_TRUNCATED)]).await;
    let container = Container::parse("ns/ci").unwrap();
    let page = backend
        .list_prefix(&ContainerPrefix::from(&container), Some("a"), 10)
        .await
        .unwrap();
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.key.as_str())
            .collect::<Vec<_>>(),
        ["a/b", "c"]
    );
    assert_eq!(page.items[0].size.as_u64(), 3);
    assert_eq!(page.next_after.as_deref(), Some("c"));
    let requests = requests.await.unwrap();
    assert_request_path(&requests[0], "GET", "/cache/");
    let line = request_line(&requests[0]);
    assert!(line.contains("prefix=ns%2Fci%2F"), "{line}");
    assert!(line.contains("max-keys=10"), "{line}");
    assert!(line.contains("start-after=ns%2Fci%2Fa"), "{line}");
}

#[tokio::test]
async fn list_prefix_has_no_cursor_when_the_listing_is_complete() {
    let (backend, requests) =
        backend(vec![(200, "application/xml", PAGE_LAST)]).await;
    let container = Container::parse("ns/ci").unwrap();
    let page = backend
        .list_prefix(&ContainerPrefix::from(&container), None, 5000)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].key, "d");
    assert_eq!(page.next_after, None);
    let requests = requests.await.unwrap();
    let line = request_line(&requests[0]);
    assert!(line.contains("max-keys=1000"), "limit is clamped: {line}");
    assert!(!line.contains("start-after"), "{line}");
}

#[tokio::test]
async fn list_prefix_marker_only_page_still_advances_the_cursor() {
    let (backend, requests) = backend(vec![
        (200, "application/xml", PAGE_MARKER_ONLY),
        (200, "application/xml", PAGE_LAST),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    let prefix = ContainerPrefix::from(&container);
    let first = backend.list_prefix(&prefix, None, 0).await.unwrap();
    assert!(first.items.is_empty());
    assert_eq!(first.next_after.as_deref(), Some(""));
    let second = backend
        .list_prefix(&prefix, first.next_after.as_deref(), 1)
        .await
        .unwrap();
    assert_eq!(second.items[0].key, "d");
    assert_eq!(second.next_after, None);
    let requests = requests.await.unwrap();
    assert!(request_line(&requests[0]).contains("max-keys=1"));
    assert!(
        request_line(&requests[1]).contains("start-after=ns%2Fci%2F"),
        "{}",
        request_line(&requests[1])
    );
}

const CHILDREN_FIRST: &str = r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><Delimiter>/</Delimiter><IsTruncated>true</IsTruncated><NextContinuationToken>tok1</NextContinuationToken><CommonPrefixes><Prefix>ns/ci/b/</Prefix></CommonPrefixes><CommonPrefixes><Prefix>ns/ci/a/</Prefix></CommonPrefixes></ListBucketResult>"#;

const CHILDREN_LAST: &str = r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><Delimiter>/</Delimiter><IsTruncated>false</IsTruncated><Contents><Key>ns/ci/top</Key><Size>1</Size></Contents><CommonPrefixes><Prefix>ns/ci/a/</Prefix></CommonPrefixes><CommonPrefixes><Prefix>ns/ci/c/</Prefix></CommonPrefixes></ListBucketResult>"#;

#[tokio::test]
async fn list_children_follows_continuation_and_returns_sorted_names() {
    let (backend, requests) = backend(vec![
        (200, "application/xml", CHILDREN_FIRST),
        (200, "application/xml", CHILDREN_LAST),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    let children = backend
        .list_children(&ContainerPrefix::from(&container))
        .await
        .unwrap();
    assert_eq!(children, ["a", "b", "c"]);
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_request_path(request, "GET", "/cache/");
        assert!(request_line(request).contains("delimiter=%2F"));
    }
    assert!(!request_line(&requests[0]).contains("continuation-token"));
    assert!(request_line(&requests[1]).contains("continuation-token=tok1"));
}

const DELETE_RESULT_OK: &str = r#"<DeleteResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"></DeleteResult>"#;

#[tokio::test]
async fn delete_prefix_lists_pages_and_deletes_each_batch() {
    let (backend, requests) = backend(vec![
        (200, "application/xml", PAGE_TRUNCATED),
        (200, "application/xml", DELETE_RESULT_OK),
        (200, "application/xml", PAGE_LAST),
        (200, "application/xml", DELETE_RESULT_OK),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    let removed = backend
        .delete_prefix(&ContainerPrefix::from(&container))
        .await
        .unwrap();
    assert_eq!(removed, 4);
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 4);
    assert_request_path(&requests[0], "GET", "/cache/");
    assert!(request_line(&requests[0]).contains("prefix=ns%2Fci%2F"));
    assert!(!request_line(&requests[0]).contains("delimiter"));
    assert_request_path(&requests[1], "POST", "/cache/");
    assert!(request_line(&requests[1]).contains("delete"));
    let body = request_body(&requests[1]);
    for key in ["ns/ci/", "ns/ci/a/b", "ns/ci/c"] {
        assert!(body.contains(&format!("<Key>{key}</Key>")), "{body}");
    }
    assert_request_path(&requests[2], "GET", "/cache/");
    assert!(request_line(&requests[2]).contains("continuation-token=tok1"));
    assert_request_path(&requests[3], "POST", "/cache/");
    assert!(request_body(&requests[3]).contains("<Key>ns/ci/d</Key>"));
}

#[tokio::test]
async fn delete_prefix_on_an_empty_prefix_removes_nothing() {
    let (backend, requests) = backend(vec![(
        200,
        "application/xml",
        r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><IsTruncated>false</IsTruncated></ListBucketResult>"#,
    )])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .delete_prefix(&ContainerPrefix::from(&container))
            .await
            .unwrap(),
        0
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request_path(&requests[0], "GET", "/cache/");
}

#[tokio::test]
async fn delete_prefix_reports_a_failed_key_as_an_error() {
    let (backend, requests) = backend(vec![
        (
            200,
            "application/xml",
            r#"<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>cache</Name><Prefix>ns/ci/</Prefix><IsTruncated>false</IsTruncated><Contents><Key>ns/ci/a</Key><Size>1</Size></Contents></ListBucketResult>"#,
        ),
        (
            200,
            "application/xml",
            r#"<DeleteResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Error><Key>ns/ci/a</Key><Code>AccessDenied</Code><Message>nope</Message></Error></DeleteResult>"#,
        ),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    let error = backend
        .delete_prefix(&ContainerPrefix::from(&container))
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("ns/ci/a"), "{error}");
    assert!(error.contains("AccessDenied"), "{error}");
    requests.await.unwrap();
}

fn header_value(request: &str, name: &str) -> Option<String> {
    request
        .split_once("\r\n\r\n")
        .unwrap()
        .0
        .lines()
        .find_map(|line| {
            let (found, value) = line.split_once(':')?;
            found
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
}

#[tokio::test]
async fn get_range_sends_suffix_and_bounded_ranges_and_returns_the_body() {
    let (backend, requests) = backend(vec![
        (200, "application/octet-stream", "ive"),
        (200, "application/octet-stream", "rch"),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .get_range(&container, "d", ByteRange::Suffix(3))
            .await
            .unwrap(),
        Some(Bytes::from_static(b"ive"))
    );
    assert_eq!(
        backend
            .get_range(&container, "d", ByteRange::Bounded(1, 4))
            .await
            .unwrap(),
        Some(Bytes::from_static(b"rch"))
    );
    let requests = requests.await.unwrap();
    assert_request_path(&requests[0], "GET", "/cache/ns/ci/d");
    assert_eq!(
        header_value(&requests[0], "range").as_deref(),
        Some("bytes=-3")
    );
    assert_request_path(&requests[1], "GET", "/cache/ns/ci/d");
    assert_eq!(
        header_value(&requests[1], "range").as_deref(),
        Some("bytes=1-3")
    );
}

#[tokio::test]
async fn get_range_past_the_end_with_an_existing_object_is_empty() {
    let (backend, requests) = backend(vec![
        (416, "application/xml", ""),
        (200, "application/xml", ""),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .get_range(&container, "d", ByteRange::Bounded(10, 12))
            .await
            .unwrap(),
        Some(Bytes::new())
    );
    let requests = requests.await.unwrap();
    assert_request_path(&requests[0], "GET", "/cache/ns/ci/d");
    assert_request_path(&requests[1], "HEAD", "/cache/ns/ci/d");
}

#[tokio::test]
async fn get_range_past_the_end_with_a_missing_object_is_none() {
    let (backend, requests) = backend(vec![
        (416, "application/xml", ""),
        (404, "application/xml", ""),
    ])
    .await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .get_range(&container, "d", ByteRange::Bounded(10, 12))
            .await
            .unwrap(),
        None
    );
    let requests = requests.await.unwrap();
    assert_request_path(&requests[1], "HEAD", "/cache/ns/ci/d");
}

#[tokio::test]
async fn get_range_on_a_missing_object_is_none() {
    let (backend, requests) = backend(vec![(404, "application/xml", "")]).await;
    let container = Container::parse("ns/ci").unwrap();
    assert_eq!(
        backend
            .get_range(&container, "d", ByteRange::Suffix(3))
            .await
            .unwrap(),
        None
    );
    let requests = requests.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request_path(&requests[0], "GET", "/cache/ns/ci/d");
}

#[tokio::test]
async fn get_range_selecting_nothing_sends_only_a_head() {
    for range in [ByteRange::Suffix(0), ByteRange::Bounded(3, 3)] {
        let (backend, requests) =
            backend(vec![(200, "application/xml", "")]).await;
        let container = Container::parse("ns/ci").unwrap();
        assert_eq!(
            backend.get_range(&container, "d", range).await.unwrap(),
            Some(Bytes::new()),
            "{range:?}"
        );
        let requests = requests.await.unwrap();
        assert_eq!(requests.len(), 1, "{range:?}");
        assert_request_path(&requests[0], "HEAD", "/cache/ns/ci/d");
        assert_eq!(header_value(&requests[0], "range"), None);
    }
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
