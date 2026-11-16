use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::Stream;
use omni_remote_cache_storage_core::{WriteStream, error::Error};
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;

use super::{TRAILER_VERSION, Trailer, TrailerCredential, to_hex};

/// Why a sealed upload failed. A route maps each to a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealFailure {
    /// More artifact bytes than the limit. Wins over a mismatch, because the
    /// stream stops at the limit and never reaches the hash check.
    TooLarge,
    /// The bytes do not hash to what the client declared.
    Mismatch,
    /// The request body failed before it ended.
    Upstream,
    /// The trailer could not be built. Not expected; a server error.
    Trailer,
}

const NONE: u8 = 0;

/// Tells a route why a sealed stream ended in an error, after the backend has
/// reported only that the write failed.
///
/// Read it only after the backend call has returned. `None` after a failed
/// write means the stream did not fail, so the backend did (a server error),
/// including a backend that dropped the stream early. A mismatch is known only
/// once every artifact byte has been forwarded, so a backend must treat the
/// stream's end as the commit point and abort on `Err`.
#[derive(Debug, Clone, Default)]
pub struct SealHandle(Arc<AtomicU8>);

impl SealHandle {
    pub fn failure(&self) -> Option<SealFailure> {
        match self.0.load(Ordering::SeqCst) {
            1 => Some(SealFailure::TooLarge),
            2 => Some(SealFailure::Mismatch),
            3 => Some(SealFailure::Upstream),
            4 => Some(SealFailure::Trailer),
            _ => None,
        }
    }

    fn set(&self, failure: SealFailure) {
        let code = match failure {
            SealFailure::TooLarge => 1,
            SealFailure::Mismatch => 2,
            SealFailure::Upstream => 3,
            SealFailure::Trailer => 4,
        };
        // The first failure is the reason.
        let _ = self.0.compare_exchange(
            NONE,
            code,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }
}

/// Everything the trailer records, all of it from the server's side.
pub struct SealParams {
    /// The client's SHA-256 of the artifact, already checked as 64 lowercase
    /// hex characters.
    pub declared_sha256: String,
    /// The most artifact bytes accepted.
    pub limit: u64,
    pub writer_credential: TrailerCredential,
    pub writer_principal: String,
    pub namespace: String,
    pub scope: Option<String>,
    /// Opaque to the server.
    pub signature: Option<String>,
    /// Read once, when the artifact has ended and its hash has matched.
    pub now: fn() -> OffsetDateTime,
}

/// Wraps an upload so that it can only end as a well-formed sealed object.
///
/// It hashes the artifact as it passes, stops at the limit, and when the body
/// ends it compares the hash with the declared one. On a match it emits the
/// trailer as the last chunk. On anything else it emits one error and then
/// nothing, so a backend that aborts on `Err` never commits a partial or
/// unverified entry. The trailer is never emitted before the check.
pub fn seal(
    body: WriteStream,
    params: SealParams,
) -> (WriteStream, SealHandle) {
    let handle = SealHandle::default();
    let sealed = Sealed {
        inner: body,
        hasher: Sha256::new(),
        total: 0,
        done: false,
        params,
        handle: handle.clone(),
    };
    (Box::pin(sealed), handle)
}

struct Sealed {
    inner: WriteStream,
    hasher: Sha256,
    total: u64,
    done: bool,
    params: SealParams,
    handle: SealHandle,
}

impl Sealed {
    fn fail(
        &mut self,
        failure: SealFailure,
        error: Error,
    ) -> Poll<Option<Result<Bytes, Error>>> {
        self.done = true;
        self.handle.set(failure);
        Poll::Ready(Some(Err(error)))
    }

    fn finish(&mut self) -> Poll<Option<Result<Bytes, Error>>> {
        let computed = to_hex(&self.hasher.finalize_reset());
        if computed != self.params.declared_sha256 {
            return self.fail(
                SealFailure::Mismatch,
                Error::custom(eyre::eyre!(
                    "the artifact does not match its declared SHA-256"
                )),
            );
        }
        let trailer = Trailer {
            version: TRAILER_VERSION,
            writer_credential: self.params.writer_credential.clone(),
            writer_principal: self.params.writer_principal.clone(),
            namespace: self.params.namespace.clone(),
            scope: self.params.scope.clone(),
            stored_at: (self.params.now)(),
            size: self.total,
            sha256: computed,
            signature: self.params.signature.clone(),
        };
        match trailer.encode() {
            Ok(bytes) => {
                self.done = true;
                Poll::Ready(Some(Ok(Bytes::from(bytes))))
            }
            Err(e) => self.fail(SealFailure::Trailer, Error::custom(e)),
        }
    }
}

impl Stream for Sealed {
    type Item = Result<Bytes, Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if this.done {
            return Poll::Ready(None);
        }
        match this.inner.as_mut().poll_next(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(chunk))) => {
                this.total = this.total.saturating_add(chunk.len() as u64);
                if this.total > this.params.limit {
                    return this.fail(
                        SealFailure::TooLarge,
                        Error::custom(eyre::eyre!(
                            "the artifact exceeds the size limit"
                        )),
                    );
                }
                this.hasher.update(&chunk);
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.fail(SealFailure::Upstream, error)
            }
            Poll::Ready(None) => this.finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt as _;

    use super::*;
    use crate::entry::{FOOTER_LEN, MAGIC};

    fn sha(bytes: &[u8]) -> String {
        to_hex(&Sha256::digest(bytes))
    }

    fn epoch() -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH
    }

    fn params(declared: String, limit: u64) -> SealParams {
        SealParams {
            declared_sha256: declared,
            limit,
            writer_credential: TrailerCredential {
                kind: "api_key".into(),
                id: "id".into(),
            },
            writer_principal: "owner".into(),
            namespace: "shared".into(),
            scope: None,
            signature: None,
            now: epoch,
        }
    }

    fn body(chunks: Vec<Result<&'static [u8], &'static str>>) -> WriteStream {
        Box::pin(futures::stream::iter(chunks.into_iter().map(|c| {
            c.map(Bytes::from_static)
                .map_err(|e| Error::custom(eyre::eyre!(e)))
        })))
    }

    async fn run(
        chunks: Vec<Result<&'static [u8], &'static str>>,
        params: SealParams,
    ) -> (Vec<Result<Bytes, Error>>, SealHandle) {
        let (stream, handle) = seal(body(chunks), params);
        (stream.collect().await, handle)
    }

    fn assert_ends_after_one_error(items: &[Result<Bytes, Error>]) {
        let errors = items.iter().filter(|i| i.is_err()).count();
        assert_eq!(errors, 1, "exactly one terminal error");
        assert!(items.last().unwrap().is_err(), "nothing follows the error");
    }

    #[tokio::test]
    async fn a_matching_upload_ends_with_the_trailer_and_never_before() {
        let artifact = b"hello, sealed world";
        let (items, handle) = run(
            vec![Ok(&artifact[..5]), Ok(&artifact[5..]), Ok(b"")],
            params(sha(artifact), 1000),
        )
        .await;
        assert_eq!(handle.failure(), None);
        let chunks: Vec<Bytes> =
            items.into_iter().map(|i| i.unwrap()).collect();
        // Artifact chunks pass through untouched and first; the trailer is
        // one extra chunk at the very end.
        assert_eq!(&chunks[0][..], &artifact[..5]);
        assert_eq!(&chunks[1][..], &artifact[5..]);
        let object: Vec<u8> = chunks.concat();
        assert!(object.starts_with(artifact));
        let parsed = Trailer::parse_tail(&object, object.len() as u64).unwrap();
        assert_eq!(parsed.trailer.size, artifact.len() as u64);
        assert_eq!(parsed.trailer.sha256, sha(artifact));
        assert_eq!(parsed.trailer.writer_principal, "owner");
        assert_eq!(parsed.trailer.namespace, "shared");
        assert_eq!(parsed.trailer.stored_at, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(parsed.sealed_len(), object.len() - artifact.len());
        assert!(object.ends_with(MAGIC));
    }

    #[tokio::test]
    async fn an_empty_artifact_is_sealed() {
        let (items, handle) = run(vec![], params(sha(b""), 0)).await;
        assert_eq!(handle.failure(), None);
        let object: Vec<u8> =
            items.into_iter().flat_map(|i| i.unwrap()).collect();
        let parsed = Trailer::parse_tail(&object, object.len() as u64).unwrap();
        assert_eq!(parsed.trailer.size, 0);
        assert!(object.len() > FOOTER_LEN);
    }

    #[tokio::test]
    async fn the_object_does_not_depend_on_chunk_boundaries() {
        let artifact: &'static [u8] = b"0123456789abcdef0123456789abcdef";
        let whole = run(vec![Ok(artifact)], params(sha(artifact), 100)).await.0;
        let split = run(
            artifact.chunks(3).map(Ok).collect(),
            params(sha(artifact), 100),
        )
        .await
        .0;
        let join = |items: Vec<Result<Bytes, Error>>| -> Vec<u8> {
            items.into_iter().flat_map(|i| i.unwrap()).collect()
        };
        assert_eq!(join(whole), join(split));
    }

    #[tokio::test]
    async fn the_cap_is_on_artifact_bytes_and_exact() {
        let artifact: &'static [u8] = b"12345678";
        // Exactly at the limit is accepted, even though the object is larger.
        let (items, handle) = run(
            vec![Ok(&artifact[..4]), Ok(&artifact[4..])],
            params(sha(artifact), 8),
        )
        .await;
        assert_eq!(handle.failure(), None);
        assert!(items.iter().all(|i| i.is_ok()));

        // One byte over fails at that chunk, with no trailer and no more input read.
        let (items, handle) = run(
            vec![Ok(&artifact[..4]), Ok(&artifact[4..]), Ok(b"9")],
            params(sha(b"123456789"), 8),
        )
        .await;
        assert_eq!(handle.failure(), Some(SealFailure::TooLarge));
        assert_ends_after_one_error(&items);
        assert_eq!(items.len(), 3, "two good chunks, then the error");

        // A single oversized chunk.
        let (items, handle) =
            run(vec![Ok(b"123456789")], params(sha(b"123456789"), 8)).await;
        assert_eq!(handle.failure(), Some(SealFailure::TooLarge));
        assert_eq!(items.len(), 1);
        assert!(items[0].is_err());
    }

    #[tokio::test]
    async fn a_mismatch_commits_no_trailer() {
        let (items, handle) = run(
            vec![Ok(b"artifact bytes")],
            params(sha(b"different bytes"), 1000),
        )
        .await;
        assert_eq!(handle.failure(), Some(SealFailure::Mismatch));
        assert_ends_after_one_error(&items);
        let delivered: Vec<u8> =
            items.into_iter().filter_map(|i| i.ok()).flatten().collect();
        assert_eq!(
            delivered, b"artifact bytes",
            "only artifact bytes, no trailer"
        );
    }

    #[tokio::test]
    async fn an_upstream_error_is_passed_on_once_and_ends_the_stream() {
        let (items, handle) = run(
            vec![Ok(b"partial"), Err("connection reset"), Ok(b"never read")],
            params(sha(b"partialnever read"), 1000),
        )
        .await;
        assert_eq!(handle.failure(), Some(SealFailure::Upstream));
        assert_ends_after_one_error(&items);
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn the_first_failure_is_the_reason() {
        // Over the limit AND not matching: the limit is hit first.
        let (_, handle) =
            run(vec![Ok(b"123456789")], params(sha(b"something else"), 4))
                .await;
        assert_eq!(handle.failure(), Some(SealFailure::TooLarge));

        let handle = SealHandle::default();
        handle.set(SealFailure::Upstream);
        handle.set(SealFailure::Mismatch);
        assert_eq!(handle.failure(), Some(SealFailure::Upstream));
        assert_eq!(SealHandle::default().failure(), None);
    }

    #[tokio::test]
    async fn the_clock_is_read_once_after_the_hash_matches() {
        use std::sync::atomic::AtomicUsize;
        static READS: AtomicUsize = AtomicUsize::new(0);
        fn counting() -> OffsetDateTime {
            READS.fetch_add(1, Ordering::SeqCst);
            OffsetDateTime::UNIX_EPOCH
        }
        let mut p = params(sha(b"x"), 10);
        p.now = counting;
        let _ = run(vec![Ok(b"x")], p).await;
        assert_eq!(READS.load(Ordering::SeqCst), 1);

        let mut p = params(sha(b"y"), 10);
        p.now = counting;
        let _ = run(vec![Ok(b"x")], p).await;
        assert_eq!(READS.load(Ordering::SeqCst), 1, "no read on a mismatch");
    }

    #[tokio::test]
    async fn a_limit_of_zero_rejects_any_byte() {
        let (items, handle) = run(vec![Ok(b"x")], params(sha(b"x"), 0)).await;
        assert_eq!(handle.failure(), Some(SealFailure::TooLarge));
        assert_eq!(items.len(), 1);
    }

    #[tokio::test]
    async fn polling_after_the_end_stays_ended() {
        for (chunks, declared) in [
            (vec![Ok(&b"ok"[..])], sha(b"ok")),
            (vec![Ok(&b"ok"[..])], sha(b"no")),
        ] {
            let (mut stream, _) = seal(body(chunks), params(declared, 100));
            while stream.next().await.is_some() {}
            for _ in 0..3 {
                assert!(stream.next().await.is_none());
            }
        }
    }

    #[tokio::test]
    async fn a_pending_input_is_waited_for_not_sealed_early() {
        use std::task::Poll;
        let mut first = true;
        let inner = futures::stream::poll_fn(move |cx| {
            if first {
                first = false;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(None)
            }
        });
        let (stream, handle) = seal(Box::pin(inner), params(sha(b""), 10));
        let items: Vec<_> = stream.collect().await;
        assert_eq!(handle.failure(), None);
        assert_eq!(items.len(), 1, "just the trailer once the input ended");
    }

    #[tokio::test]
    async fn an_oversized_trailer_is_a_server_failure_not_a_commit() {
        let mut p = params(sha(b"x"), 10);
        p.writer_principal = "p".repeat(crate::entry::MAX_TRAILER_LEN);
        let (items, handle) = run(vec![Ok(b"x")], p).await;
        assert_eq!(handle.failure(), Some(SealFailure::Trailer));
        assert_ends_after_one_error(&items);
    }
}
