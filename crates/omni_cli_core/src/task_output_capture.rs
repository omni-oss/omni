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
/// `<scratch>/logs/<bs58(project)>/<bs58(task)>-<seq>`.
///
/// The base58 of two different names can differ only by case (`aa` and `aH`
/// encode to `8Qp` and `8QP`), which a case-insensitive filesystem treats as one
/// file. The unique `seq` keeps every capture file distinct whatever the names,
/// so two drains never truncate one file. The path is recorded with the capture
/// itself, never derived from a label.
pub fn capture_path(
    scratch_dir: &Path,
    project: &str,
    task: &str,
    seq: u64,
) -> PathBuf {
    scratch_dir
        .join("logs")
        .join(bs58::encode(project.as_bytes()).into_string())
        .join(format!("{}-{seq}", bs58::encode(task.as_bytes()).into_string()))
}

/// Fully drain `reader`. When `to_file` is `Some`, the bytes are written there
/// and `File(path)` is returned; otherwise the bytes are discarded and `Sink`
/// is returned.
///
/// The reader is **always** drained to completion, even on write errors, so the
/// producing child process can never deadlock waiting for its output to be
/// consumed.
pub async fn drain(
    mut reader: impl AsyncRead + Unpin,
    to_file: Option<PathBuf>,
) -> std::io::Result<CaptureTarget> {
    match to_file {
        Some(path) => {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            // Exclusive: a capture file is never shared or truncated.
            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .await?;
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
