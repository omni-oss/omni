use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use omni_task_output_logs::LogsDisplay;
use tokio::io::{AsyncRead, AsyncWriteExt, BufWriter};

/// Where a task's captured output was routed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    /// Bytes were written to this file and may be replayed later.
    File(PathBuf),
    /// Bytes were discarded.
    Sink,
}

static CAPTURE_SEQ: AtomicU64 = AtomicU64::new(0);

/// A process-unique, monotonic capture sequence number.
pub fn next_capture_seq() -> u64 {
    CAPTURE_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Per-capture file path:
/// `<scratch>/logs/<bs58(project)>/<bs58(task)>-<pid>-<seq>`.
///
/// The base58 of two different names can differ only by case (`aa` and `aH`
/// encode to `8Qp` and `8QP`), which a case-insensitive filesystem treats as one
/// file. The unique `pid` and `seq` keep every capture file distinct whatever the
/// names, even for two omni processes sharing one scratch directory, so two
/// drains never truncate one file. The path is recorded with the capture
/// itself, never derived from a label.
pub fn capture_path(
    scratch_dir: &Path,
    project: &str,
    task: &str,
    seq: u64,
) -> PathBuf {
    capture_path_for_process(
        scratch_dir,
        project,
        task,
        std::process::id(),
        seq,
    )
}

/// [`capture_path`] with an explicit process id.
pub fn capture_path_for_process(
    scratch_dir: &Path,
    project: &str,
    task: &str,
    pid: u32,
    seq: u64,
) -> PathBuf {
    scratch_dir
        .join("logs")
        .join(bs58::encode(project.as_bytes()).into_string())
        .join(format!(
            "{}-{pid}-{seq}",
            bs58::encode(task.as_bytes()).into_string()
        ))
}

const MAX_EXISTING_RETRIES: u32 = 16;

/// `<name>-<pid>-<seq>` -> `<name>-<pid>-<seq + 1>`.
fn next_sequence_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    let (head, seq) = name.rsplit_once('-')?;
    let seq: u64 = seq.parse().ok()?;
    Some(path.with_file_name(format!("{head}-{}", seq.checked_add(1)?)))
}

/// Fully drain `reader`. When `to_file` is `Some`, the bytes are written there
/// and `File(path)` is returned; otherwise the bytes are discarded and `Sink`
/// is returned.
///
/// The reader is **always** drained to completion, even when the file cannot be
/// created or written, so the producing child process can never deadlock or hit
/// a broken pipe waiting for its output to be consumed.
pub async fn drain(
    mut reader: impl AsyncRead + Unpin,
    to_file: Option<PathBuf>,
) -> std::io::Result<CaptureTarget> {
    match to_file {
        Some(path) => {
            let mut path = path;
            let file = async {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut attempt = 0;
                loop {
                    // Exclusive: a capture file is never shared or truncated.
                    match tokio::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                        .await
                    {
                        Err(e)
                            if e.kind()
                                == std::io::ErrorKind::AlreadyExists
                                && attempt < MAX_EXISTING_RETRIES =>
                        {
                            // A stale file (crash, shared volume): move on to
                            // the next sequence number.
                            match next_sequence_path(&path) {
                                Some(next) => path = next,
                                None => return Err(e),
                            }
                            attempt += 1;
                        }
                        other => return other,
                    }
                }
            }
            .await;
            let file = match file {
                Ok(file) => file,
                Err(e) => {
                    let _ =
                        tokio::io::copy(&mut reader, &mut tokio::io::sink())
                            .await;
                    return Err(e);
                }
            };
            let mut writer = BufWriter::new(file);
            let copy_result =
                tokio::io::copy(&mut reader, &mut writer).await.map(|_| ());
            // Flush regardless of copy outcome.
            let flush_result = writer.flush().await;
            copy_result.and(flush_result)?;
            Ok(CaptureTarget::File(path))
        }
        None => {
            tokio::io::copy(&mut reader, &mut tokio::io::sink()).await?;
            Ok(CaptureTarget::Sink)
        }
    }
}

/// Whether captured output should be displayed given the resolved display facet
/// and whether the task failed.
pub fn should_display(facet: LogsDisplay, failed: bool) -> bool {
    facet.should_show(failed)
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[test]
    fn capture_path_is_nested_and_unique_per_sequence() {
        let root = Path::new("/scratch");
        let a = capture_path(root, "proj", "task", 1);
        assert_eq!(a, capture_path(root, "proj", "task", 1));
        assert!(a.starts_with(Path::new("/scratch").join("logs")));
        assert_ne!(a, capture_path(root, "proj", "task", 2));
        assert_ne!(
            capture_path(root, "proj", "task-a", 1),
            capture_path(root, "proj", "task-b", 1)
        );
    }

    #[test]
    fn names_whose_encodings_differ_only_by_case_get_distinct_files() {
        let root = Path::new("/scratch");
        let (aa, ah) = (
            bs58::encode(b"aa").into_string(),
            bs58::encode(b"aH").into_string(),
        );
        assert_eq!(aa.to_lowercase(), ah.to_lowercase());
        let first = capture_path(root, "p", "aa", next_capture_seq());
        let second = capture_path(root, "p", "aH", next_capture_seq());
        assert_ne!(
            first.to_string_lossy().to_lowercase(),
            second.to_string_lossy().to_lowercase(),
            "a case-insensitive filesystem must not see one file"
        );
    }

    #[tokio::test]
    async fn two_captures_with_colliding_encodings_keep_their_own_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let first = capture_path(dir.path(), "p", "aa", next_capture_seq());
        let second = capture_path(dir.path(), "p", "aH", next_capture_seq());
        let one = drain(&b"first"[..], Some(first.clone())).await.unwrap();
        let two = drain(&b"second"[..], Some(second.clone())).await.unwrap();
        assert_eq!(one, CaptureTarget::File(first.clone()));
        assert_eq!(two, CaptureTarget::File(second.clone()));
        assert_eq!(tokio::fs::read(&first).await.unwrap(), b"first");
        assert_eq!(tokio::fs::read(&second).await.unwrap(), b"second");
    }

    #[tokio::test]
    async fn an_existing_capture_file_is_never_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("taken");
        tokio::fs::write(&path, b"keep").await.unwrap();
        assert!(drain(&b"x"[..], Some(path.clone())).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"keep");
    }

    #[tokio::test]
    async fn an_open_failure_still_drains_the_reader_to_completion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("taken");
        tokio::fs::write(&path, b"keep").await.unwrap();
        let (mut tx, rx) = tokio::io::duplex(64);
        let drain = tokio::spawn(drain(rx, Some(path.clone())));
        // Far more than the pipe buffer: blocks forever unless drained.
        let payload = vec![7u8; 64 * 1024];
        tx.write_all(&payload)
            .await
            .expect("producer must not block or break");
        drop(tx);
        assert!(drain.await.unwrap().is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"keep");
    }

    #[tokio::test]
    async fn an_unopenable_parent_still_drains_the_reader() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        tokio::fs::write(&blocker, b"x").await.unwrap();
        let (mut tx, rx) = tokio::io::duplex(64);
        let drain =
            tokio::spawn(drain(rx, Some(blocker.join("child").join("out"))));
        tx.write_all(&vec![1u8; 64 * 1024]).await.unwrap();
        drop(tx);
        assert!(drain.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn same_named_captures_from_different_processes_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let a = capture_path_for_process(dir.path(), "p", "t", 100, 0);
        let b = capture_path_for_process(dir.path(), "p", "t", 200, 0);
        assert_ne!(a, b);
        assert!(matches!(
            drain(&b"one"[..], Some(a.clone())).await,
            Ok(CaptureTarget::File(_))
        ));
        assert!(matches!(
            drain(&b"two"[..], Some(b.clone())).await,
            Ok(CaptureTarget::File(_))
        ));
        assert_eq!(tokio::fs::read(&a).await.unwrap(), b"one");
        assert_eq!(tokio::fs::read(&b).await.unwrap(), b"two");
        assert_eq!(
            capture_path(dir.path(), "p", "t", 5),
            capture_path_for_process(
                dir.path(),
                "p",
                "t",
                std::process::id(),
                5
            )
        );
    }

    #[tokio::test]
    async fn stale_capture_files_are_skipped_by_taking_the_next_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let p0 = capture_path_for_process(dir.path(), "p", "t", 7, 0);
        let p1 = capture_path_for_process(dir.path(), "p", "t", 7, 1);
        std::fs::create_dir_all(p0.parent().unwrap()).unwrap();
        std::fs::write(&p0, b"stale0").unwrap();
        std::fs::write(&p1, b"stale1").unwrap();
        let p2 = capture_path_for_process(dir.path(), "p", "t", 7, 2);

        let target = drain(&b"fresh"[..], Some(p0.clone())).await.unwrap();
        assert_eq!(target, CaptureTarget::File(p2.clone()));
        assert_eq!(std::fs::read(&p2).unwrap(), b"fresh");
        assert_eq!(std::fs::read(&p0).unwrap(), b"stale0");
        assert_eq!(std::fs::read(&p1).unwrap(), b"stale1");
    }

    #[tokio::test]
    async fn drain_to_file_writes_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("out.log");
        let data = b"hello world".to_vec();

        let target = drain(&data[..], Some(path.clone())).await.unwrap();

        assert_eq!(target, CaptureTarget::File(path.clone()));
        let contents = tokio::fs::read(&path).await.unwrap();
        assert_eq!(contents, data);
    }

    #[tokio::test]
    async fn drain_to_sink_discards_bytes() {
        let data = b"discard me".to_vec();
        let target = drain(&data[..], None).await.unwrap();
        assert_eq!(target, CaptureTarget::Sink);
    }

    #[test]
    fn should_display_truth_table() {
        assert!(should_display(LogsDisplay::All, false));
        assert!(should_display(LogsDisplay::All, true));
        assert!(!should_display(LogsDisplay::Failed, false));
        assert!(should_display(LogsDisplay::Failed, true));
        assert!(!should_display(LogsDisplay::Never, false));
        assert!(!should_display(LogsDisplay::Never, true));
    }
}
