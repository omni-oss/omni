//! Dependency-neutral local on-disk storage primitives.
//!
//! Nothing here knows about tasks, archives or planning. Consumers own their
//! key scheme and record format; this layer only provides the mechanics:
//!
//! * crash-safe publishing of a staged directory under a content-addressed
//!   name ([`publish_staged_dir`]), where a sentinel file marks completeness,
//! * atomic single-file replacement ([`write_atomic`]),
//! * bounded reads ([`read_bounded`]),
//! * reclaiming staging directories orphaned by crashed processes
//!   ([`reclaim_stale_staging_dirs`]),
//! * a caller-chosen failure policy ([`WritePolicy`]).
//!
//! The advisory lock helpers live in [`lock`].

pub(crate) mod lock;
mod policy;
mod publish;
mod records;

pub use policy::WritePolicy;
pub use publish::{
    STAGING_PREFIX, publish_staged_dir, reclaim_stale_staging_dirs,
    staging_dir_for,
};
pub use records::{read_bounded, write_atomic};

/// Returns whether a directory entry name denotes internal scratch state
/// (staging dirs or the lock folder) rather than a real record.
pub fn is_internal_entry(name: &str) -> bool {
    name.starts_with('.')
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use super::*;

    /// A consumer with its own key and record type and no task dependencies.
    struct NoteStore<'a>(&'a Path);

    impl NoteStore<'_> {
        fn dir(&self, key: u32) -> std::path::PathBuf {
            self.0.join(format!("note-{key}"))
        }

        async fn put(&self, key: u32, text: &str) -> std::io::Result<()> {
            let dir = self.dir(key);
            let staging = staging_dir_for(&dir);
            tokio::fs::create_dir_all(&staging).await?;
            write_atomic(&staging.join("note.txt"), text.as_bytes()).await?;
            publish_staged_dir(&staging, &dir, &dir.join("note.txt"), false)
                .await
        }

        async fn get(&self, key: u32) -> std::io::Result<String> {
            let bytes =
                read_bounded(&self.dir(key).join("note.txt"), 1024).await?;
            String::from_utf8(bytes)
                .map_err(|e| std::io::Error::other(e.to_string()))
        }
    }

    #[tokio::test]
    async fn test_consumer_round_trips_and_keeps_first_writer() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = NoteStore(tmp.path());
        store.put(1, "first").await.expect("put");
        store.put(1, "second").await.expect("put again");
        assert_eq!(store.get(1).await.expect("get"), "first");
    }

    #[tokio::test]
    async fn test_concurrent_publish_of_same_key_is_safe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = NoteStore(tmp.path());
        let results =
            futures_join(&store, (0..8).map(|i| format!("v{i}"))).await;
        assert!(results.iter().all(|r| r.is_ok()));
        assert!(store.get(7).await.expect("get").starts_with('v'));
        let leftovers = std::fs::read_dir(tmp.path())
            .expect("read_dir")
            .filter(|e| {
                e.as_ref()
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(STAGING_PREFIX)
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    async fn futures_join(
        store: &NoteStore<'_>,
        texts: impl Iterator<Item = String>,
    ) -> Vec<std::io::Result<()>> {
        let mut out = Vec::new();
        let futs: Vec<_> = texts
            .map(|t| async move { store.put(7, &t).await })
            .collect();
        for f in futs {
            out.push(f.await);
        }
        out
    }

    #[tokio::test]
    async fn test_partial_destination_without_sentinel_is_replaced() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = NoteStore(tmp.path());
        std::fs::create_dir_all(store.dir(2)).expect("mk partial");
        std::fs::write(store.dir(2).join("junk"), "x").expect("junk");
        store.put(2, "ok").await.expect("put");
        assert_eq!(store.get(2).await.expect("get"), "ok");
        assert!(!store.dir(2).join("junk").exists());
    }

    #[tokio::test]
    async fn test_corrupt_or_oversized_record_is_an_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = tmp.path().join("big");
        std::fs::write(&p, vec![0u8; 100]).expect("write");
        let err = read_bounded(&p, 10).await.expect_err("too big");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(read_bounded(&p, 100).await.expect("exact").len(), 100);
        assert!(read_bounded(&tmp.path().join("none"), 10).await.is_err());
    }

    #[tokio::test]
    async fn test_write_atomic_removes_temp_file_when_rename_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("dest");
        std::fs::create_dir(&dest).unwrap();

        assert!(write_atomic(&dest, b"x").await.is_err());

        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("dest")]);
    }

    #[tokio::test]
    async fn test_write_atomic_replaces_and_leaves_no_temp_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let p = tmp.path().join("db");
        write_atomic(&p, b"a").await.expect("write");
        write_atomic(&p, b"b").await.expect("write");
        assert_eq!(std::fs::read(&p).expect("read"), b"b");
        assert_eq!(std::fs::read_dir(tmp.path()).expect("ls").count(), 1);
        assert!(
            write_atomic(&tmp.path().join("no/dir"), b"x")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_reclaim_removes_only_stale_staging_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let stale = tmp.path().join(format!("{STAGING_PREFIX}crashed"));
        let real = tmp.path().join("entry");
        std::fs::create_dir_all(&stale).expect("mk");
        std::fs::create_dir_all(&real).expect("mk");
        reclaim_stale_staging_dirs(tmp.path(), Duration::from_secs(3600)).await;
        assert!(stale.exists(), "fresh staging dir survives");
        reclaim_stale_staging_dirs(tmp.path(), Duration::ZERO).await;
        assert!(!stale.exists());
        assert!(real.exists());
        reclaim_stale_staging_dirs(&tmp.path().join("missing"), Duration::ZERO)
            .await;
    }

    #[test]
    fn test_write_policy() {
        let err: Result<(), String> = Err("boom".into());
        assert!(WritePolicy::Required.apply(err.clone()).is_err());
        assert!(WritePolicy::BestEffort.apply(err).is_ok());
        assert!(WritePolicy::Required.apply::<String>(Ok(())).is_ok());
    }

    #[test]
    fn test_internal_entries() {
        assert!(is_internal_entry(".locks"));
        assert!(is_internal_entry(".staging-x"));
        assert!(!is_internal_entry("proj"));
    }
}
