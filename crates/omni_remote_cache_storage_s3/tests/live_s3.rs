//! Create-only behaviour against a real S3 API server.
//!
//! Skipped unless `OMNI_TEST_S3_ENDPOINT` is set (with `OMNI_TEST_S3_ACCESS_KEY`
//! and `OMNI_TEST_S3_SECRET_KEY`). It uses any S3 compatible server (MinIO,
//! RustFS, SeaweedFS, real S3) and a fresh bucket per test.

use std::sync::Arc;

use bytes::Bytes;
use omni_remote_cache_storage_core::{
    ByteRange, Container, ContainerPrefix, RemoteCacheStorageBackend,
    SaveOutcome, WriteStream, error::Error,
};
use omni_remote_cache_storage_s3::{BasicS3Config, S3CacheBackend};

struct Live {
    backend: Arc<S3CacheBackend>,
}

async fn live() -> Option<Live> {
    let endpoint = std::env::var("OMNI_TEST_S3_ENDPOINT").ok()?;
    let access = std::env::var("OMNI_TEST_S3_ACCESS_KEY").ok()?;
    let secret = std::env::var("OMNI_TEST_S3_SECRET_KEY").ok()?;
    let bucket = format!(
        "omni-live-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let credentials = aws_credential_types::Credentials::new(
        &access, &secret, None, None, "Static",
    );
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .credentials_provider(credentials)
        .region(aws_config::Region::new("us-east-1"))
        .endpoint_url(&endpoint)
        .load()
        .await;
    let conf = aws_sdk_s3::config::Builder::from(&sdk)
        .force_path_style(true)
        .build();
    aws_sdk_s3::Client::from_conf(conf)
        .create_bucket()
        .bucket(&bucket)
        .send()
        .await
        .expect("create the test bucket");
    let backend = S3CacheBackend::from_basic_config(&BasicS3Config::new(
        access,
        secret,
        endpoint,
        bucket,
        "us-east-1".into(),
        true,
    ))
    .await;
    Some(Live {
        backend: Arc::new(backend),
    })
}

fn c(name: &str) -> Container {
    Container::parse(name).unwrap()
}

fn stream(chunks: Vec<&'static [u8]>) -> WriteStream {
    Box::pin(futures_stream(chunks))
}

fn futures_stream(
    chunks: Vec<&'static [u8]>,
) -> impl futures_core_stream::Stream<Item = Result<Bytes, Error>> + Send {
    tokio_stream::iter(chunks.into_iter().map(|c| Ok(Bytes::from_static(c))))
}

use tokio_stream as futures_core_stream;

#[tokio::test]
async fn a_second_write_never_replaces_the_first() {
    let Some(live) = live().await else { return };
    let b = &live.backend;
    assert_eq!(
        b.save_if_absent(&c("t/o/w/e"), "k", Bytes::from("first"))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
    assert_eq!(
        b.save_if_absent(&c("t/o/w/e"), "k", Bytes::from("second"))
            .await
            .unwrap(),
        SaveOutcome::AlreadyExists
    );
    assert_eq!(
        b.get(&c("t/o/w/e"), "k").await.unwrap().unwrap(),
        Bytes::from("first")
    );
    // Streamed writes behave the same.
    assert_eq!(
        b.save_stream_if_absent(&c("t/o/w/e"), "s", stream(vec![b"ab", b"cd"]))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
    assert_eq!(
        b.save_stream_if_absent(&c("t/o/w/e"), "s", stream(vec![b"zz"]))
            .await
            .unwrap(),
        SaveOutcome::AlreadyExists
    );
    assert_eq!(
        b.get(&c("t/o/w/e"), "s").await.unwrap().unwrap(),
        Bytes::from("abcd")
    );
}

#[tokio::test]
async fn concurrent_first_writers_agree_on_one_winner() {
    let Some(live) = live().await else { return };
    let mut tasks = Vec::new();
    for i in 0..16 {
        let b = live.backend.clone();
        tasks.push(tokio::spawn(async move {
            let value = Bytes::from(format!("writer-{i}"));
            let outcome = b
                .save_if_absent(&c("race"), "k", value.clone())
                .await
                .unwrap();
            (outcome, value)
        }));
    }
    let mut winners = Vec::new();
    for t in tasks {
        let (outcome, value) = t.await.unwrap();
        if outcome == SaveOutcome::Stored {
            winners.push(value);
        }
    }
    assert_eq!(winners.len(), 1, "exactly one writer may win: {winners:?}");
    assert_eq!(
        live.backend.get(&c("race"), "k").await.unwrap().unwrap(),
        winners[0]
    );
}

#[tokio::test]
async fn a_failing_stream_commits_nothing() {
    let Some(live) = live().await else { return };
    let b = &live.backend;
    let bad: WriteStream = Box::pin(tokio_stream::iter(vec![
        Ok(Bytes::from_static(b"partial")),
        Err(Error::custom(std::io::Error::other("upstream failed"))),
    ]));
    assert!(b.save_stream_if_absent(&c("fail"), "k", bad).await.is_err());
    assert!(b.get(&c("fail"), "k").await.unwrap().is_none());
    // The key is still free.
    assert_eq!(
        b.save_if_absent(&c("fail"), "k", Bytes::from("ok"))
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
}

#[tokio::test]
async fn ranges_prefixes_and_deletes_stay_inside_their_container() {
    let Some(live) = live().await else { return };
    let b = &live.backend;
    for (container, key) in [("ci", "a"), ("ci", "b"), ("ci2", "a")] {
        b.save_if_absent(&c(container), key, Bytes::from("0123456789"))
            .await
            .unwrap();
    }
    assert_eq!(
        b.get_range(&c("ci"), "a", ByteRange::Suffix(3))
            .await
            .unwrap()
            .unwrap(),
        Bytes::from("789")
    );
    assert_eq!(
        b.get_range(&c("ci"), "a", ByteRange::Bounded(2, 5))
            .await
            .unwrap()
            .unwrap(),
        Bytes::from("234")
    );
    let prefix = ContainerPrefix::parse("ci/").unwrap();
    let page = b.list_prefix(&prefix, None, 1000).await.unwrap();
    let keys: Vec<_> = page.items.iter().map(|i| i.key.clone()).collect();
    assert_eq!(keys, ["a", "b"], "ci must not list ci2");
    assert_eq!(b.delete_prefix(&prefix).await.unwrap(), 2);
    assert!(
        b.exists(&c("ci2"), "a").await.unwrap(),
        "ci2 survives a ci prefix delete"
    );
    assert!(!b.exists(&c("ci"), "a").await.unwrap());
}
