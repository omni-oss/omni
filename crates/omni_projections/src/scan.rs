use std::path::Path;

use system_traits::{FsMetadataAsync, FsReadDirAsync};

use crate::error::Result;
use crate::routing::ScannedEntry;

/// The system surface needed to enumerate a source tree.
pub trait ScanSys: FsReadDirAsync + FsMetadataAsync + Sync {}
impl<T> ScanSys for T where T: FsReadDirAsync + FsMetadataAsync + Sync {}

/// Recursively enumerate every entry under `source_root`, expressed relative to
/// it. Directory symlinks are recorded but never descended into, so the scan
/// cannot loop through cyclic links or wander outside the tree.
pub async fn scan_source<S: ScanSys>(
    sys: &S,
    source_root: &Path,
) -> Result<Vec<ScannedEntry>> {
    let mut out = Vec::new();
    let mut stack = vec![source_root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for path in sys.fs_read_dir_async(&dir).await? {
            let rel = path
                .strip_prefix(source_root)
                .map(Path::to_path_buf)
                .unwrap_or_else(|_| path.clone());
            let is_symlink = sys.fs_is_symlink_no_err_async(&path).await;
            let is_dir = sys.fs_is_dir_no_err_async(&path).await;

            out.push(ScannedEntry {
                rel,
                is_dir,
                is_symlink,
            });

            if is_dir && !is_symlink {
                stack.push(path);
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_traits::impls::RealSys;

    #[tokio::test]
    async fn scans_nested_tree_relative_to_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("sub/b.txt"), b"b").unwrap();

        let mut entries = scan_source(&RealSys, root).await.unwrap();
        entries.sort_by(|a, b| a.rel.cmp(&b.rel));

        let rels: Vec<_> = entries
            .iter()
            .map(|e| e.rel.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(rels, vec!["a.txt", "sub", "sub/b.txt"]);
    }

    #[tokio::test]
    async fn does_not_descend_into_symlinked_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::fs::write(root.join("real/inner.txt"), b"x").unwrap();
        let link = root.join("linked");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("real"), &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(root.join("real"), &link).unwrap();

        let entries = scan_source(&RealSys, root).await.unwrap();
        let has_linked_inner = entries.iter().any(|e| {
            e.rel.to_string_lossy().replace('\\', "/") == "linked/inner.txt"
        });
        assert!(!has_linked_inner, "must not descend into a symlinked dir");
        let linked = entries
            .iter()
            .find(|e| e.rel.to_string_lossy() == "linked")
            .expect("symlinked dir recorded");
        assert!(linked.is_symlink);
    }
}
