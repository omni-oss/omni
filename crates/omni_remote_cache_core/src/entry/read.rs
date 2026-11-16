use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::Stream;
use omni_remote_cache_storage_core::{
    ByteRange, Container, ReadStream, RemoteCacheStorageBackend, error::Error,
};

use super::{EntryError, Parsed, TAIL_READ_LEN, Trailer};

/// A sealed object that passed [`Trailer::parse_tail`], with what was read to
/// prove it.
#[derive(Debug, Clone)]
pub struct ValidEntry {
    pub parsed: Parsed,
    /// The last bytes of the object, as read.
    pub tail: Bytes,
    pub object_len: u64,
}

/// What a stored object turned out to be.
#[derive(Debug)]
pub enum EntryRead {
    /// Nothing is stored under the key.
    Absent,
    /// Something is stored but it is not a well-formed sealed entry (for
    /// example one written before trailers existed). Callers treat it as a miss.
    Invalid(EntryError),
    Valid(Box<ValidEntry>),
}

/// Reads and checks the end of the object stored at `key`.
///
/// `GET` and `HEAD` both use this, so they agree about which entries exist. It
/// costs one ranged read, plus a `size` call only when the object is longer than
/// the read.
pub async fn read_entry(
    backend: &(dyn RemoteCacheStorageBackend + Send + Sync),
    container: &Container,
    key: &str,
) -> Result<EntryRead, Error> {
    let Some(tail) = backend
        .get_range(container, key, ByteRange::Suffix(TAIL_READ_LEN))
        .await?
    else {
        return Ok(EntryRead::Absent);
    };
    // A short answer means the object is that short; otherwise ask how long it is.
    let object_len = if (tail.len() as u64) < TAIL_READ_LEN {
        tail.len() as u64
    } else {
        match backend.size(container, key).await? {
            Some(size) => size.as_u64(),
            None => return Ok(EntryRead::Absent),
        }
    };
    Ok(match Trailer::parse_tail(&tail, object_len) {
        Ok(parsed) => EntryRead::Valid(Box::new(ValidEntry {
            parsed,
            tail,
            object_len,
        })),
        Err(error) => EntryRead::Invalid(error),
    })
}

/// Streams exactly the artifact out of a sealed object and checks, before the
/// last artifact byte is released, that the object was the one [`read_entry`]
/// described.
///
/// The first `size - 1` bytes pass through. The last artifact byte and
/// everything after it (at most a trailer and a footer) are held back. When the
/// object ends, the held trailer must equal the end of the tail that was read
/// first and the object must have the length that was read; only then is the last
/// byte released. Anything else ends the body with an error **before** the
/// response's declared `Content-Length` is satisfied, so the HTTP layer aborts the
/// transfer instead of finishing it. (Releasing every artifact byte first would
/// not work: an HTTP/1 server stops reading the body once it has sent
/// `Content-Length` bytes, so a check after the last byte would never be seen.)
/// A purge and re-publish between the two reads therefore can never produce a
/// body paired with the wrong trailer.
pub fn checked_body(stream: ReadStream, entry: &ValidEntry) -> ReadStream {
    Box::pin(Checked {
        inner: stream,
        size: entry.parsed.trailer.size,
        passed: 0,
        last: None,
        held: Vec::new(),
        entry: entry.clone(),
        done: false,
    })
}

struct Checked {
    inner: ReadStream,
    size: u64,
    /// Artifact bytes seen so far, including the held-back last one.
    passed: u64,
    /// The final artifact byte, kept until the whole object has been checked.
    last: Option<u8>,
    held: Vec<u8>,
    entry: ValidEntry,
    done: bool,
}

impl Checked {
    fn fail(
        &mut self,
        why: &'static str,
    ) -> Poll<Option<Result<Bytes, Error>>> {
        self.done = true;
        Poll::Ready(Some(Err(Error::custom(eyre::eyre!(why)))))
    }
}

impl Stream for Checked {
    type Item = Result<Bytes, Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        // Bytes that may be released at once: all but the last artifact byte.
        let releasable = this.size.saturating_sub(1);
        loop {
            if this.done {
                return Poll::Ready(None);
            }
            match this.inner.as_mut().poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Err(error))) => {
                    this.done = true;
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(Some(Ok(chunk))) => {
                    let take = usize::try_from(
                        releasable
                            .saturating_sub(this.passed)
                            .min(chunk.len() as u64),
                    )
                    .expect("bounded by the chunk length");
                    let body = chunk.slice(..take);
                    this.passed += take as u64;
                    let mut rest = chunk.slice(take..);
                    if this.size > 0
                        && this.passed == releasable
                        && this.last.is_none()
                        && let Some(&byte) = rest.first()
                    {
                        this.last = Some(byte);
                        this.passed += 1;
                        rest = rest.slice(1..);
                    }
                    if !rest.is_empty() {
                        this.held.extend_from_slice(&rest);
                        if this.held.len() > this.entry.parsed.sealed_len() {
                            return this.fail(
                                "the stored object is longer than it was when read",
                            );
                        }
                    }
                    if !body.is_empty() {
                        return Poll::Ready(Some(Ok(body)));
                    }
                }
                Poll::Ready(None) => {
                    let intact = this.passed == this.size
                        && this.passed + this.held.len() as u64
                            == this.entry.object_len
                        && this
                            .entry
                            .parsed
                            .matches_tail(&this.entry.tail, &this.held);
                    if !intact {
                        return this.fail(
                            "the stored object changed while it was being read",
                        );
                    }
                    this.done = true;
                    return Poll::Ready(
                        this.last
                            .take()
                            .map(|byte| Ok(Bytes::from(vec![byte]))),
                    );
                }
            }
        }
    }
}
