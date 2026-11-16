use std::num::NonZeroUsize;

use bytes::Bytes;
use futures::StreamExt as _;
use omni_remote_cache_core::entry::{
    EntryError, EntryRead, SealParams, TAIL_READ_LEN, TrailerCredential,
    checked_body, read_entry, seal,
};
use omni_remote_cache_storage_core::{
    Container, RemoteCacheStorageBackend, SaveOutcome, decorators::LruCached,
    error::Error,
};
use omni_remote_cache_storage_memory::InMemoryBackend;
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;

fn container() -> Container {
    Container::parse("t/o/w/e").unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn params(artifact: &[u8], stored_at: fn() -> OffsetDateTime) -> SealParams {
    SealParams {
        declared_sha256: hex(&Sha256::digest(artifact)),
        limit: u64::MAX,
        writer_credential: TrailerCredential {
            kind: "api_key".into(),
            id: "id".into(),
        },
        writer_principal: "owner".into(),
        namespace: "shared".into(),
        scope: None,
        signature: None,
        now: stored_at,
    }
}

fn epoch() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH
}

fn later() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(60)
}

/// Seals `artifact` in 8 KiB chunks and stores it.
async fn store(
    backend: &(dyn RemoteCacheStorageBackend + Send + Sync),
    key: &str,
    artifact: &[u8],
    stored_at: fn() -> OffsetDateTime,
) {
    let chunks: Vec<Result<Bytes, Error>> = artifact
        .chunks(8192)
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    let (stream, _) = seal(
        Box::pin(futures::stream::iter(chunks)),
        params(artifact, stored_at),
    );
    assert_eq!(
        backend
            .save_stream_if_absent(&container(), key, stream)
            .await
            .unwrap(),
        SaveOutcome::Stored
    );
}

async fn artifact_of(
    backend: &(dyn RemoteCacheStorageBackend + Send + Sync),
    key: &str,
) -> Result<Vec<u8>, String> {
    let EntryRead::Valid(entry) =
        read_entry(backend, &container(), key).await.unwrap()
    else {
        return Err("not valid".into());
    };
    let stream = backend
        .get_stream(&container(), key)
        .await
        .unwrap()
        .unwrap();
    let mut body = checked_body(stream, &entry);
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
    }
    assert_eq!(out.len() as u64, entry.parsed.trailer.size);
    Ok(out)
}

fn artifacts() -> Vec<(&'static str, Vec<u8>)> {
    let mut trailer_like = b"payload".to_vec();
    trailer_like.extend_from_slice(&1u32.to_be_bytes());
    trailer_like.extend_from_slice(b"OMNIENT1");
    vec![
        ("empty", vec![]),
        ("one byte", vec![9]),
        ("small", b"hello".to_vec()),
        ("looks like a footer", trailer_like),
        // Longer than the tail read, so the object length comes from `size`.
        (
            "larger than the tail read",
            (0..250_000u32).map(|i| i as u8).collect(),
        ),
        ("exactly the tail read", vec![5; TAIL_READ_LEN as usize]),
    ]
}

#[tokio::test]
async fn every_artifact_reads_back_exactly_and_without_the_trailer() {
    for (name, artifact) in artifacts() {
        let backend = InMemoryBackend::new();
        store(&backend, "k", &artifact, epoch).await;
        assert_eq!(
            artifact_of(&backend, "k").await.unwrap(),
            artifact,
            "{name}"
        );
    }
}

#[tokio::test]
async fn the_same_entries_read_back_through_the_lru_from_cache_and_after_eviction()
 {
    for (name, artifact) in artifacts() {
        let inner = std::sync::Arc::new(InMemoryBackend::new());
        // Written through the cache: the sealed bytes are cached when small enough.
        let cached = LruCached::new(
            ArcBackend(inner.clone()),
            NonZeroUsize::new(4).unwrap(),
        );
        store(&cached, "k", &artifact, epoch).await;
        assert_eq!(
            artifact_of(&cached, "k").await.unwrap(),
            artifact,
            "{name}: cached"
        );
        // A cold cache over the same storage reads the same bytes.
        let cold =
            LruCached::new(ArcBackend(inner), NonZeroUsize::new(4).unwrap());
        assert_eq!(
            artifact_of(&cold, "k").await.unwrap(),
            artifact,
            "{name}: cold"
        );
    }
}

#[tokio::test]
async fn absent_legacy_and_malformed_objects_are_not_valid_entries() {
    let backend = InMemoryBackend::new();
    assert!(matches!(
        read_entry(&backend, &container(), "missing").await.unwrap(),
        EntryRead::Absent
    ));
    for (name, bytes, want) in [
        ("empty", &b""[..], EntryError::TooShort),
        ("eleven bytes", b"01234567890", EntryError::TooShort),
        (
            "a legacy archive",
            b"\x1f\x8b\x08\x00 an old entry with no footer at all",
            EntryError::BadMagic,
        ),
    ] {
        backend
            .save_if_absent(&container(), name, Bytes::copy_from_slice(bytes))
            .await
            .unwrap();
        match read_entry(&backend, &container(), name).await.unwrap() {
            EntryRead::Invalid(got) => assert_eq!(got, want, "{name}"),
            other => panic!("{name}: {other:?}"),
        }
    }
    // A long legacy object takes the `size` path and is still not valid.
    backend
        .save_if_absent(&container(), "long", Bytes::from(vec![1u8; 200_000]))
        .await
        .unwrap();
    assert!(matches!(
        read_entry(&backend, &container(), "long").await.unwrap(),
        EntryRead::Invalid(EntryError::BadMagic)
    ));
}

#[tokio::test]
async fn an_object_replaced_between_the_two_reads_aborts_the_body() {
    let backend = InMemoryBackend::new();
    store(&backend, "k", b"the original artifact", epoch).await;
    let EntryRead::Valid(entry) =
        read_entry(&backend, &container(), "k").await.unwrap()
    else {
        panic!("valid")
    };

    // Same artifact bytes and length, published again later: only the trailer differs.
    backend.delete(&container(), "k").await.unwrap();
    store(&backend, "k", b"the original artifact", later).await;
    let stream = backend
        .get_stream(&container(), "k")
        .await
        .unwrap()
        .unwrap();
    let results: Vec<_> = checked_body(stream, &entry).collect().await;
    assert!(results.last().unwrap().is_err(), "a re-publish is detected");
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);

    // A different artifact of the same length.
    backend.delete(&container(), "k").await.unwrap();
    store(&backend, "k", b"THE ORIGINAL ARTIFACT", epoch).await;
    let stream = backend
        .get_stream(&container(), "k")
        .await
        .unwrap()
        .unwrap();
    let results: Vec<_> = checked_body(stream, &entry).collect().await;
    assert!(
        results.last().unwrap().is_err(),
        "other bytes, same trailer region?"
    );
}

#[tokio::test]
async fn a_longer_or_shorter_object_than_was_read_aborts_the_body() {
    let backend = InMemoryBackend::new();
    store(&backend, "k", b"short", epoch).await;
    let EntryRead::Valid(entry) =
        read_entry(&backend, &container(), "k").await.unwrap()
    else {
        panic!("valid")
    };
    for (name, replacement) in [
        ("a much longer artifact", vec![7u8; 100_000]),
        ("an empty artifact", vec![]),
    ] {
        backend.delete(&container(), "k").await.unwrap();
        store(&backend, "k", &replacement, epoch).await;
        let stream = backend
            .get_stream(&container(), "k")
            .await
            .unwrap()
            .unwrap();
        let results: Vec<_> = checked_body(stream, &entry).collect().await;
        assert!(results.last().unwrap().is_err(), "{name}");
    }
}

#[tokio::test]
async fn a_truncated_object_and_a_failing_read_end_with_one_error() {
    let backend = InMemoryBackend::new();
    store(&backend, "k", b"0123456789", epoch).await;
    let EntryRead::Valid(entry) =
        read_entry(&backend, &container(), "k").await.unwrap()
    else {
        panic!("valid")
    };
    let object = backend.get(&container(), "k").await.unwrap().unwrap();

    // The stored object lost its last bytes.
    let cut = object.slice(..object.len() - 3);
    let stream: omni_remote_cache_storage_core::ReadStream =
        Box::pin(futures::stream::iter([Ok(cut)]));
    let results: Vec<_> = checked_body(stream, &entry).collect().await;
    assert!(results.last().unwrap().is_err());
    assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);

    // The storage read itself fails part way.
    let stream: omni_remote_cache_storage_core::ReadStream =
        Box::pin(futures::stream::iter([
            Ok(object.slice(..4)),
            Err(Error::custom(eyre::eyre!("disk error"))),
            Ok(object.slice(4..)),
        ]));
    let results: Vec<_> = checked_body(stream, &entry).collect().await;
    assert_eq!(
        results.len(),
        2,
        "the good chunk, then the error, then nothing"
    );
    assert!(results[1].is_err());
}

#[tokio::test]
async fn chunk_boundaries_do_not_matter_even_inside_the_trailer() {
    let backend = InMemoryBackend::new();
    let artifact: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    store(&backend, "k", &artifact, epoch).await;
    let EntryRead::Valid(entry) =
        read_entry(&backend, &container(), "k").await.unwrap()
    else {
        panic!("valid")
    };
    let object = backend.get(&container(), "k").await.unwrap().unwrap();
    for size in [1, 2, 7, 999, 1000, 1001, object.len()] {
        let chunks: Vec<Result<Bytes, Error>> = object
            .chunks(size)
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect();
        let stream: omni_remote_cache_storage_core::ReadStream =
            Box::pin(futures::stream::iter(chunks));
        let mut out = Vec::new();
        let mut body = checked_body(stream, &entry);
        while let Some(chunk) = body.next().await {
            out.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(out, artifact, "chunk size {size}");
    }
}

/// Pulls a body the way an HTTP/1 server does for a response that declared
/// `Content-Length: size`: it stops as soon as that many bytes were delivered and
/// never polls again, so a check that only runs at the stream's end is never seen.
async fn deliver_like_http1(
    mut body: omni_remote_cache_storage_core::ReadStream,
    size: u64,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    while (out.len() as u64) < size {
        match body.next().await {
            Some(chunk) => {
                out.extend_from_slice(&chunk.map_err(|e| e.to_string())?)
            }
            None => return Err("ended before the declared length".into()),
        }
    }
    Ok(out)
}

#[tokio::test]
async fn a_swapped_object_is_caught_before_the_declared_length_is_delivered() {
    // Same artifact bytes, so the same size, published again later: only the
    // trailer differs. An HTTP/1 server that stops at `Content-Length` must still
    // see the failure, because the last artifact byte is not released early.
    let backend = InMemoryBackend::new();
    let artifact: &[u8] = b"the very same artifact";
    store(&backend, "k", artifact, epoch).await;
    let EntryRead::Valid(entry) =
        read_entry(&backend, &container(), "k").await.unwrap()
    else {
        panic!("valid")
    };
    let size = entry.parsed.trailer.size;

    // Control: the object that was read is delivered in full.
    let stream = backend
        .get_stream(&container(), "k")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        deliver_like_http1(checked_body(stream, &entry), size)
            .await
            .unwrap(),
        artifact
    );

    backend.delete(&container(), "k").await.unwrap();
    store(&backend, "k", artifact, later).await;
    let stream = backend
        .get_stream(&container(), "k")
        .await
        .unwrap()
        .unwrap();
    let delivered =
        deliver_like_http1(checked_body(stream, &entry), size).await;
    assert!(
        delivered.is_err(),
        "the swap must abort the transfer: {delivered:?}"
    );

    // A different artifact of the same length is caught the same way.
    backend.delete(&container(), "k").await.unwrap();
    store(&backend, "k", b"THE VERY SAME ARTIFACT", epoch).await;
    let stream = backend
        .get_stream(&container(), "k")
        .await
        .unwrap()
        .unwrap();
    assert!(
        deliver_like_http1(checked_body(stream, &entry), size)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn every_chunk_size_delivers_the_artifact_in_full_to_an_http1_style_reader()
 {
    let backend = InMemoryBackend::new();
    for artifact in [&b"x"[..], b"ab", b"0123456789"] {
        store(&backend, "k", artifact, epoch).await;
        let EntryRead::Valid(entry) =
            read_entry(&backend, &container(), "k").await.unwrap()
        else {
            panic!("valid")
        };
        let object = backend.get(&container(), "k").await.unwrap().unwrap();
        for chunk in [1, 2, 3, 5, 7, object.len()] {
            let chunks: Vec<Result<Bytes, Error>> = object
                .chunks(chunk)
                .map(|c| Ok(Bytes::copy_from_slice(c)))
                .collect();
            let stream: omni_remote_cache_storage_core::ReadStream =
                Box::pin(futures::stream::iter(chunks));
            let got = deliver_like_http1(
                checked_body(stream, &entry),
                artifact.len() as u64,
            )
            .await
            .unwrap();
            assert_eq!(got, artifact, "chunk size {chunk}");
        }
        backend.delete(&container(), "k").await.unwrap();
    }
}

/// Lets a shared in-memory backend sit under more than one cache.
struct ArcBackend(std::sync::Arc<InMemoryBackend>);

#[async_trait::async_trait]
impl RemoteCacheStorageBackend for ArcBackend {
    async fn get(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<Bytes>, Error> {
        self.0.get(c, k).await
    }
    async fn exists(&self, c: &Container, k: &str) -> Result<bool, Error> {
        self.0.exists(c, k).await
    }
    async fn get_stream(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<omni_remote_cache_storage_core::ReadStream>, Error> {
        self.0.get_stream(c, k).await
    }
    async fn get_range(
        &self,
        c: &Container,
        k: &str,
        r: omni_remote_cache_storage_core::ByteRange,
    ) -> Result<Option<Bytes>, Error> {
        self.0.get_range(c, k, r).await
    }
    async fn list_prefix(
        &self,
        p: &omni_remote_cache_storage_core::ContainerPrefix,
        a: Option<&str>,
        l: u32,
    ) -> Result<omni_remote_cache_storage_core::PrefixPage, Error> {
        self.0.list_prefix(p, a, l).await
    }
    async fn list_children(
        &self,
        p: &omni_remote_cache_storage_core::ContainerPrefix,
    ) -> Result<Vec<String>, Error> {
        self.0.list_children(p).await
    }
    async fn delete_prefix(
        &self,
        p: &omni_remote_cache_storage_core::ContainerPrefix,
    ) -> Result<u64, Error> {
        self.0.delete_prefix(p).await
    }
    async fn save_if_absent(
        &self,
        c: &Container,
        k: &str,
        b: Bytes,
    ) -> Result<SaveOutcome, Error> {
        self.0.save_if_absent(c, k, b).await
    }
    async fn save_stream_if_absent(
        &self,
        c: &Container,
        k: &str,
        s: omni_remote_cache_storage_core::WriteStream,
    ) -> Result<SaveOutcome, Error> {
        self.0.save_stream_if_absent(c, k, s).await
    }
    async fn delete(&self, c: &Container, k: &str) -> Result<(), Error> {
        self.0.delete(c, k).await
    }
    async fn size(
        &self,
        c: &Container,
        k: &str,
    ) -> Result<Option<bytesize::ByteSize>, Error> {
        self.0.size(c, k).await
    }
}
