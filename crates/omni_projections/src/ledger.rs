use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use system_traits::{FsCreateDirAllAsync, FsReadAsync, FsWriteAsync};

use crate::apply::{ApplierSys, ResolvedKind};
use crate::error::Result;

/// The applied-link ledger: derived, machine-local state (not committed).
///
/// Versioned like the lockfile: the `version` tag selects the payload, so the
/// schema can evolve without breaking older on-disk files.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "version", rename_all = "kebab-case")]
pub enum Ledger {
    #[serde(rename = "1.0.0")]
    V1_0_0(LedgerV1_0_0),
}

impl Default for Ledger {
    fn default() -> Self {
        Self::V1_0_0(LedgerV1_0_0::default())
    }
}

impl Ledger {
    /// Build a current-version ledger from a set of links.
    pub fn from_links(links: Vec<LedgerLink>) -> Self {
        Self::V1_0_0(LedgerV1_0_0 { links })
    }

    /// The recorded links, regardless of version.
    pub fn links(&self) -> &[LedgerLink] {
        let Ledger::V1_0_0(v) = self;
        &v.links
    }

    /// Mutable access to the recorded links.
    pub fn links_mut(&mut self) -> &mut Vec<LedgerLink> {
        let Ledger::V1_0_0(v) = self;
        &mut v.links
    }
}

/// The `1.0.0` ledger payload.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerV1_0_0 {
    #[serde(default)]
    pub links: Vec<LedgerLink>,
}

/// One recorded link, keyed by the projection source `id`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LedgerLink {
    pub source_id: String,
    /// Workspace-relative destination.
    pub dest: String,
    /// What the link points at (the source).
    pub target: String,
    pub kind: ResolvedKind,
    pub source_pin: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<String>,
}

/// Load the ledger from `ledger_path`. A missing, corrupt, or unparseable
/// ledger is treated as empty (config stays authoritative) and never triggers a
/// destructive teardown. The caller owns where the ledger lives.
pub async fn load<S: FsReadAsync + Sync>(
    sys: &S,
    ledger_path: &Path,
) -> Ledger {
    match sys.fs_read_async(ledger_path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            log::warn!(
                "projection ledger at {} is unreadable, treating as empty: {e}",
                ledger_path.display()
            );
            Ledger::default()
        }),
        Err(_) => Ledger::default(),
    }
}

/// Persist the ledger to `ledger_path`, creating its parent directory if needed.
pub async fn save<S>(sys: &S, ledger_path: &Path, ledger: &Ledger) -> Result<()>
where
    S: FsWriteAsync + FsCreateDirAllAsync + Sync,
{
    if let Some(parent) = ledger_path.parent() {
        sys.fs_create_dir_all_async(parent).await?;
    }
    let bytes = serde_json::to_vec_pretty(ledger)?;
    sys.fs_write_async(ledger_path, &bytes).await?;
    Ok(())
}

/// Compute a `local` source pin: a content hash over the matched files (sorted
/// by relative path), including each path so renames are reflected.
pub async fn local_source_pin<S: FsReadAsync + Sync>(
    sys: &S,
    source_root: &Path,
    matched_rel: &[PathBuf],
) -> Result<String> {
    let mut sorted = matched_rel.to_vec();
    sorted.sort();

    let mut buf = Vec::new();
    for rel in &sorted {
        let bytes = sys.fs_read_async(&source_root.join(rel)).await?;
        buf.extend_from_slice(rel.to_string_lossy().as_bytes());
        buf.push(0);
        buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(&bytes);
    }

    Ok(hex(&omni_hasher::default::hash_bytes(&buf)?))
}

/// The outcome of a teardown operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeardownReport {
    pub removed: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Remove only the ledger-recorded links for `source_id`. A `copy`-kind link is
/// removed only when its destination content still matches the source (else it
/// is kept and a warning is recorded), so user edits are never silently lost.
pub async fn unlink<S>(
    sys: &S,
    workspace_root: &Path,
    ledger: &mut Ledger,
    source_id: &str,
    clean_backups: bool,
) -> Result<TeardownReport>
where
    S: ApplierSys + FsReadAsync,
{
    let mut report = TeardownReport::default();
    let mut kept = Vec::new();

    for link in std::mem::take(ledger.links_mut()) {
        if link.source_id != source_id {
            kept.push(link);
            continue;
        }

        let dest = workspace_root.join(&link.dest);

        if link.kind == ResolvedKind::Copy
            && copy_was_modified(sys, &dest, &link.target).await
        {
            report.warnings.push(format!(
                "kept modified copy at {} (edited since projection)",
                dest.display()
            ));
            kept.push(link);
            continue;
        }

        if remove_path(sys, &dest).await? {
            report.removed.push(dest.clone());
            prune_empty_ancestors(sys, workspace_root, &dest).await;
        }

        if clean_backups {
            if let Some(backup) = &link.backup {
                let backup_path = workspace_root.join(backup);
                let _ = sys.fs_remove_file_async(&backup_path).await;
            }
        }
    }

    *ledger.links_mut() = kept;
    Ok(report)
}

/// Remove ledger-recorded links whose destination is now a dangling symlink
/// (the target no longer resolves). Never touches unrecorded files.
pub async fn prune<S>(
    sys: &S,
    workspace_root: &Path,
    ledger: &mut Ledger,
) -> Result<TeardownReport>
where
    S: ApplierSys,
{
    let mut report = TeardownReport::default();
    let mut kept = Vec::new();

    for link in std::mem::take(ledger.links_mut()) {
        let dest = workspace_root.join(&link.dest);
        let is_symlink = sys.fs_is_symlink_no_err_async(&dest).await;
        let resolves = sys.fs_exists_no_err_async(&dest).await;

        if is_symlink && !resolves {
            if remove_path(sys, &dest).await? {
                report.removed.push(dest);
            }
        } else {
            kept.push(link);
        }
    }

    *ledger.links_mut() = kept;
    Ok(report)
}

pub(crate) async fn copy_was_modified<S: FsReadAsync + Sync>(
    sys: &S,
    dest: &Path,
    source: &str,
) -> bool {
    let dest_bytes = match sys.fs_read_async(dest).await {
        Ok(b) => b,
        Err(_) => return false,
    };
    let source_bytes = match sys.fs_read_async(Path::new(source)).await {
        Ok(b) => b,
        // Source gone: cannot prove it is unmodified, so keep it to be safe.
        Err(_) => return true,
    };
    dest_bytes != source_bytes
}

pub(crate) async fn remove_path<S: ApplierSys>(
    sys: &S,
    path: &Path,
) -> Result<bool> {
    if !sys.fs_is_symlink_no_err_async(path).await
        && !sys.fs_exists_no_err_async(path).await
    {
        return Ok(false);
    }

    if sys.fs_is_dir_no_err_async(path).await
        && !sys.fs_is_symlink_no_err_async(path).await
    {
        sys.fs_remove_dir_all_async(path).await?;
    } else if sys.fs_remove_file_async(path).await.is_err() {
        sys.fs_remove_dir_all_async(path).await?;
    }

    Ok(true)
}

/// Remove now-empty ancestor directories of a removed dest, stopping at the
/// first non-empty directory or at the workspace root.
pub(crate) async fn prune_empty_ancestors<S: ApplierSys>(
    sys: &S,
    workspace_root: &Path,
    dest: &Path,
) {
    let mut current = dest.parent().map(Path::to_path_buf);

    while let Some(dir) = current {
        if dir == workspace_root || !dir.starts_with(workspace_root) {
            break;
        }
        match sys.fs_read_dir_async(&dir).await {
            Ok(entries) if entries.is_empty() => {
                if sys.fs_remove_dir_all_async(&dir).await.is_err() {
                    break;
                }
            }
            _ => break,
        }
        current = dir.parent().map(Path::to_path_buf);
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_traits::impls::RealSys;

    #[test]
    fn ledger_serde_round_trips() {
        let ledger = Ledger::from_links(vec![LedgerLink {
            source_id: "team-ai-skills".to_string(),
            dest: ".cursor/rules/main.md".to_string(),
            target: "/abs/src/main.md".to_string(),
            kind: ResolvedKind::Symlink,
            source_pin: "deadbeef".to_string(),
            backup: None,
        }]);
        let json = serde_json::to_string(&ledger).unwrap();
        let back: Ledger = serde_json::from_str(&json).unwrap();
        assert_eq!(ledger, back);
    }

    #[test]
    fn ledger_wire_shape_is_version_tagged() {
        // The versioned enum must serialize to the same `{version, links}`
        // object the pre-enum struct used, so existing files keep parsing.
        let json = r#"{"version":"1.0.0","links":[]}"#;
        let ledger: Ledger = serde_json::from_str(json).unwrap();
        assert_eq!(ledger, Ledger::default());
        assert_eq!(serde_json::to_string(&ledger).unwrap(), json);
    }

    #[tokio::test]
    async fn corrupt_ledger_is_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("links.json");
        std::fs::write(&path, b"{ not valid json").unwrap();

        let ledger = load(&RealSys, &path).await;
        assert!(ledger.links().is_empty());
    }

    #[tokio::test]
    async fn absent_ledger_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = load(&RealSys, &dir.path().join("links.json")).await;
        assert_eq!(ledger, Ledger::default());
    }

    #[tokio::test]
    async fn unlink_never_touches_unrecorded_files() {
        let dir = tempfile::tempdir().unwrap();
        let unrecorded = dir.path().join("keep.txt");
        std::fs::write(&unrecorded, b"user data").unwrap();

        let mut ledger = Ledger::default();
        let report =
            unlink(&RealSys, dir.path(), &mut ledger, "nonexistent", false)
                .await
                .unwrap();

        assert!(report.removed.is_empty());
        assert!(unrecorded.exists());
    }

    #[tokio::test]
    async fn unlink_preserves_modified_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"original").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"user edited this").unwrap();

        let mut ledger = Ledger::from_links(vec![LedgerLink {
            source_id: "id".to_string(),
            dest: "dest.txt".to_string(),
            target: source.to_string_lossy().into_owned(),
            kind: ResolvedKind::Copy,
            source_pin: "x".to_string(),
            backup: None,
        }]);

        let report = unlink(&RealSys, dir.path(), &mut ledger, "id", false)
            .await
            .unwrap();

        assert!(report.removed.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(dest.exists(), "modified copy must be preserved");
        assert_eq!(ledger.links().len(), 1, "link kept in ledger");
    }

    #[tokio::test]
    async fn unlink_removes_unmodified_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"same").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"same").unwrap();

        let mut ledger = Ledger::from_links(vec![LedgerLink {
            source_id: "id".to_string(),
            dest: "dest.txt".to_string(),
            target: source.to_string_lossy().into_owned(),
            kind: ResolvedKind::Copy,
            source_pin: "x".to_string(),
            backup: None,
        }]);

        let report = unlink(&RealSys, dir.path(), &mut ledger, "id", false)
            .await
            .unwrap();

        assert_eq!(report.removed.len(), 1);
        assert!(!dest.exists());
        assert!(ledger.links().is_empty());
    }
}
