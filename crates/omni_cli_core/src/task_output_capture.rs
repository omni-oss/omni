use std::path::{Path, PathBuf};

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

/// Deterministic per-task capture-file path:
/// `<scratch>/logs/<bs58(project)>/<bs58(task)>`.
pub fn capture_path(scratch_dir: &Path, project: &str, task: &str) -> PathBuf {
    scratch_dir
        .join("logs")
        .join(bs58::encode(project.as_bytes()).into_string())
        .join(bs58::encode(task.as_bytes()).into_string())
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
            let file = tokio::fs::File::create(&path).await?;
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
    fn capture_path_is_deterministic_and_nested() {
        let root = Path::new("/scratch");
        let a = capture_path(root, "proj", "task");
        let b = capture_path(root, "proj", "task");
        assert_eq!(a, b);
        assert!(a.starts_with(Path::new("/scratch").join("logs")));
        // Different tasks produce different leaf names.
        assert_ne!(
            capture_path(root, "proj", "task-a"),
            capture_path(root, "proj", "task-b")
        );
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
