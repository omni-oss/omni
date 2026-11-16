//! Expiry of isolated entries (RFC 0030, section 4.6).
//!
//! Isolated entries are one writer's untrusted scratch space, so they expire by
//! age since `stored_at`. S3 and R2 deployments use a bucket lifecycle rule on
//! the `isolated/` prefix; this is for backends with no such rule (local disk
//! and memory). Shared entries are never touched.

use omni_remote_cache_storage_core::{
    Container, ContainerPrefix, LIST_PAGE_SIZE, error::Error,
};
use time::OffsetDateTime;

use crate::{
    entry::{EntryRead, read_entry},
    state::DynStorageBackend,
};

/// What one pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Isolated objects looked at.
    pub scanned: u64,
    /// Entries removed because they were stored before the cutoff.
    pub deleted: u64,
    /// Objects with no valid trailer (so no age) or at an unexpected path;
    /// left alone.
    pub skipped: u64,
    /// Objects that could not be read or deleted; the pass went on.
    pub failed: u64,
    /// Where the next pass should start (`None` when this pass reached the end
    /// of `isolated/`), so a bounded pass makes progress through a large store
    /// instead of rescanning the same first objects.
    pub next_cursor: Option<String>,
}

/// Removes isolated entries stored before `cutoff`. A key is
/// `{tenant}/{org}/{ws}/{env}/{principal}/{scope}/{digest}` below `isolated/`;
/// anything with another shape, or without a readable trailer, is skipped and
/// counted, never deleted. Pass `max_objects` to bound one pass: the next
/// pass resumes at `report.next_cursor`. A failure on one object is counted
/// and does not end the pass; only a failed listing does.
///
/// A delete can race a re-publish of the same digest (the entry is removed
/// first, then written again between the trailer read and the delete) and
/// remove the new entry. The cost is one cache miss and a rebuild.
pub async fn sweep_isolated(
    storage: &DynStorageBackend,
    cutoff: OffsetDateTime,
    max_objects: Option<u64>,
    start_after: Option<String>,
) -> Result<SweepReport, Error> {
    let prefix = ContainerPrefix::parse("isolated/")
        .expect("`isolated/` is a valid prefix");
    let mut report = SweepReport::default();
    let mut after: Option<String> = start_after;
    loop {
        let page = storage
            .list_prefix(&prefix, after.as_deref(), LIST_PAGE_SIZE)
            .await?;
        for item in page.items {
            if max_objects.is_some_and(|max| report.scanned >= max) {
                return Ok(report);
            }
            report.next_cursor = Some(item.key.clone());
            report.scanned += 1;
            // Six directory segments and the digest.
            let segments: Vec<&str> = item.key.split('/').collect();
            let [t, o, w, e, p, s, digest] = segments[..] else {
                report.skipped += 1;
                continue;
            };
            let Ok(container) =
                Container::parse(format!("isolated/{t}/{o}/{w}/{e}/{p}/{s}"))
            else {
                report.skipped += 1;
                continue;
            };
            match read_entry(storage, &container, digest).await {
                Ok(EntryRead::Valid(entry))
                    if entry.parsed.trailer.stored_at < cutoff =>
                {
                    match storage.delete(&container, digest).await {
                        Ok(()) => report.deleted += 1,
                        Err(error) => {
                            tracing::warn!(%error, "could not delete an expired isolated entry");
                            report.failed += 1;
                        }
                    }
                }
                Ok(EntryRead::Valid(_) | EntryRead::Absent) => {}
                Ok(EntryRead::Invalid(_)) => report.skipped += 1,
                Err(error) => {
                    tracing::warn!(%error, "could not read an isolated entry");
                    report.failed += 1;
                }
            }
        }
        match page.next_after {
            Some(next) => after = Some(next),
            None => {
                report.next_cursor = None;
                return Ok(report);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use omni_remote_cache_storage_core::{
        RemoteCacheStorageBackend as _, SaveOutcome, error::Error as E,
    };
    use omni_remote_cache_storage_memory::InMemoryBackend;
    use sha2::{Digest as _, Sha256};
    use time::Duration;

    use super::*;
    use crate::entry::{SealParams, TrailerCredential, seal};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    async fn put(
        backend: &InMemoryBackend,
        container: &str,
        key: &str,
        namespace: &str,
    ) {
        let body = b"artifact";
        let (stream, _) = seal(
            Box::pin(futures::stream::iter(vec![Ok::<_, E>(
                Bytes::from_static(body),
            )])),
            SealParams {
                declared_sha256: hex(&Sha256::digest(body)),
                limit: u64::MAX,
                writer_credential: TrailerCredential {
                    kind: "api_key".into(),
                    id: "id".into(),
                },
                writer_principal: "p".into(),
                namespace: namespace.into(),
                scope: Some("s".into()),
                signature: None,
                now: OffsetDateTime::now_utc,
            },
        );
        assert_eq!(
            backend
                .save_stream_if_absent(
                    &Container::parse(container).unwrap(),
                    key,
                    stream
                )
                .await
                .unwrap(),
            SaveOutcome::Stored
        );
    }

    #[tokio::test]
    async fn only_old_isolated_entries_with_a_valid_trailer_are_removed() {
        let backend = InMemoryBackend::new();
        // The cutoff is set in the future for "everything stored so far is old"
        // and in the past for "nothing is old".
        put(&backend, "isolated/t/o/w/e/p/s", "d1", "isolated").await;
        put(&backend, "isolated/t/o/w/e/p/s", "d2", "isolated").await;
        put(&backend, "shared/t/o/w/e", "d3", "shared").await;
        // Junk: not a sealed entry, and a path of the wrong depth.
        backend
            .save_if_absent(
                &Container::parse("isolated/t/o/w/e/p/s").unwrap(),
                "junk",
                Bytes::from_static(b"no trailer"),
            )
            .await
            .unwrap();
        backend
            .save_if_absent(
                &Container::parse("isolated/t/o").unwrap(),
                "odd",
                Bytes::from_static(b"x"),
            )
            .await
            .unwrap();
        let storage: &DynStorageBackend = &backend;

        // Nothing is older than a cutoff in the past.
        let none = sweep_isolated(
            storage,
            OffsetDateTime::now_utc() - Duration::days(7),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            (none.scanned, none.deleted, none.skipped, none.failed),
            (4, 0, 2, 0)
        );
        assert_eq!(none.next_cursor, None, "reached the end");

        // Everything stored so far is older than a cutoff in the future.
        let all = sweep_isolated(
            storage,
            OffsetDateTime::now_utc() + Duration::days(1),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            (all.scanned, all.deleted, all.skipped, all.failed),
            (4, 2, 2, 0)
        );
        assert!(
            !backend
                .exists(
                    &Container::parse("isolated/t/o/w/e/p/s").unwrap(),
                    "d1"
                )
                .await
                .unwrap()
        );
        // Shared entries, junk and odd paths survive.
        for (c, k) in [
            ("shared/t/o/w/e", "d3"),
            ("isolated/t/o/w/e/p/s", "junk"),
            ("isolated/t/o", "odd"),
        ] {
            assert!(
                backend
                    .exists(&Container::parse(c).unwrap(), k)
                    .await
                    .unwrap(),
                "{c}/{k}"
            );
        }
    }

    #[tokio::test]
    async fn bounded_passes_resume_and_reach_every_object() {
        let backend = InMemoryBackend::new();
        for i in 0..5 {
            put(
                &backend,
                "isolated/t/o/w/e/p/s",
                &format!("d{i}"),
                "isolated",
            )
            .await;
        }
        let storage: &DynStorageBackend = &backend;
        // Everything is old, two objects per pass: the cursor must carry the
        // scan forward even though earlier objects were not deleted yet.
        let cutoff = OffsetDateTime::now_utc() + Duration::days(1);
        let mut cursor = None;
        let mut deleted = 0;
        for _ in 0..10 {
            let report = sweep_isolated(storage, cutoff, Some(2), cursor)
                .await
                .unwrap();
            deleted += report.deleted;
            cursor = report.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(deleted, 5);
    }

    #[tokio::test]
    async fn live_objects_ahead_of_old_ones_do_not_starve_them() {
        let backend = InMemoryBackend::new();
        let storage: &DynStorageBackend = &backend;
        // Three keys, none old yet: a bounded pass visits two of them and
        // reports where to resume rather than starting over.
        for i in 0..3 {
            put(
                &backend,
                "isolated/t/o/w/e/p/s",
                &format!("d{i}"),
                "isolated",
            )
            .await;
        }
        let past = OffsetDateTime::now_utc() - Duration::days(1);
        let first = sweep_isolated(storage, past, Some(2), None).await.unwrap();
        assert_eq!((first.scanned, first.deleted), (2, 0));
        let cursor = first.next_cursor.clone().expect("a cursor");
        let second = sweep_isolated(storage, past, Some(2), Some(cursor))
            .await
            .unwrap();
        assert_eq!(second.scanned, 1, "only the object after the cursor");
        assert_eq!(second.next_cursor, None);
    }

    #[tokio::test]
    async fn a_valid_entry_at_a_wrong_depth_is_never_deleted() {
        let backend = InMemoryBackend::new();
        put(&backend, "isolated/t/o/w/e/p/s/extra", "d1", "isolated").await;
        let storage: &DynStorageBackend = &backend;
        let report = sweep_isolated(
            storage,
            OffsetDateTime::now_utc() + Duration::days(1),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!((report.deleted, report.skipped), (0, 1));
    }
}
