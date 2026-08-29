use std::collections::HashSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
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

/// How a teardown disposes of a link's recorded backup file.
#[derive(
    Serialize,
    Deserialize,
    JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum BackupHandling {
    /// Remove the link; leave any recorded `.bak.<ts>` in place.
    #[default]
    Leave,
    /// Remove the link and delete the recorded `.bak.<ts>`.
    Clean,
    /// Remove the link and rename the recorded `.bak.<ts>` back to the
    /// link's original destination.
    Restore,
}

/// The outcome of a teardown operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeardownReport {
    pub removed: Vec<PathBuf>,
    pub restored: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Remove only the ledger-recorded links for `source_id`. A `copy`-kind link is
/// removed only when its destination content still matches the source (else it
/// is kept and a warning is recorded), so user edits are never silently lost.
/// `handling` chooses what happens to each removed link's recorded backup.
pub async fn unlink<S>(
    sys: &S,
    workspace_root: &Path,
    ledger: &mut Ledger,
    source_id: &str,
    handling: BackupHandling,
) -> Result<TeardownReport>
where
    S: ApplierSys + FsReadAsync,
{
    let links = std::mem::take(ledger.links_mut());
    let (kept, report) = teardown_matching(
        sys,
        workspace_root,
        links,
        |l| l.source_id == source_id,
        handling,
        false,
    )
    .await?;
    *ledger.links_mut() = kept;
    Ok(report)
}

/// Tear down and de-record every ledger link whose `source_id` is not in
/// `keep_ids`. Shares `unlink`'s safe-teardown body: a `copy`-kind link is
/// removed only when its destination still matches the source, else it is kept
/// and a warning is recorded. Recorded backups are always left in place.
pub async fn retain_sources<S>(
    sys: &S,
    workspace_root: &Path,
    ledger: &mut Ledger,
    keep_ids: &HashSet<&str>,
) -> Result<TeardownReport>
where
    S: ApplierSys + FsReadAsync,
{
    let links = std::mem::take(ledger.links_mut());
    let (kept, report) = teardown_matching(
        sys,
        workspace_root,
        links,
        |l| !keep_ids.contains(l.source_id.as_str()),
        BackupHandling::Leave,
        false,
    )
    .await?;
    *ledger.links_mut() = kept;
    Ok(report)
}

/// Read-only preview of [`retain_sources`]: report the destinations that would
/// be removed and the modified copies that would be kept, without touching disk
/// or mutating the ledger.
pub async fn plan_retain_sources<S>(
    sys: &S,
    workspace_root: &Path,
    ledger: &Ledger,
    keep_ids: &HashSet<&str>,
) -> Result<TeardownReport>
where
    S: ApplierSys + FsReadAsync,
{
    let (_, report) = teardown_matching(
        sys,
        workspace_root,
        ledger.links().to_vec(),
        |l| !keep_ids.contains(l.source_id.as_str()),
        BackupHandling::Leave,
        true,
    )
    .await?;
    Ok(report)
}

/// The shared teardown body behind [`unlink`], [`retain_sources`], and their
/// dry-run preview. Consumes `links`, removing every one the `should_teardown`
/// predicate selects, and returns the links kept alongside the report. A
/// modified `copy` is always kept and warned on. On `dry_run` nothing on disk
/// changes and every link is returned as kept.
async fn teardown_matching<S, P>(
    sys: &S,
    workspace_root: &Path,
    links: Vec<LedgerLink>,
    should_teardown: P,
    handling: BackupHandling,
    dry_run: bool,
) -> Result<(Vec<LedgerLink>, TeardownReport)>
where
    S: ApplierSys + FsReadAsync,
    P: Fn(&LedgerLink) -> bool,
{
    let mut report = TeardownReport::default();
    let mut kept = Vec::new();

    for link in links {
        if !should_teardown(&link) {
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
            if handling == BackupHandling::Restore
                && let Some(backup) = &link.backup
            {
                report.warnings.push(format!(
                    "backup {backup} not restored: destination holds an edited copy"
                ));
            }
            kept.push(link);
            continue;
        }

        if dry_run {
            report.removed.push(dest);
            kept.push(link);
            continue;
        }

        let removed = remove_path(sys, &dest).await?;
        if removed {
            report.removed.push(dest.clone());
        }

        match handling {
            BackupHandling::Leave => {}
            BackupHandling::Clean => {
                if let Some(backup) = &link.backup {
                    let backup_path = workspace_root.join(backup);
                    let _ = sys.fs_remove_file_async(&backup_path).await;
                }
            }
            BackupHandling::Restore => {
                restore_backup(sys, workspace_root, &link, &dest, &mut report)
                    .await;
            }
        }

        if removed {
            prune_empty_ancestors(sys, workspace_root, &dest).await;
        }
    }

    Ok((kept, report))
}

/// Rename a removed link's recorded backup back to its original destination.
/// Non-destructive: a missing backup, an occupied destination, or a failed
/// rename degrades to a warning and a skip, never a clobber and never an error.
async fn restore_backup<S: ApplierSys>(
    sys: &S,
    workspace_root: &Path,
    link: &LedgerLink,
    dest: &Path,
    report: &mut TeardownReport,
) {
    let Some(backup) = &link.backup else {
        return;
    };
    let backup_path = workspace_root.join(backup);

    if !sys.fs_is_symlink_no_err_async(&backup_path).await
        && !sys.fs_exists_no_err_async(&backup_path).await
    {
        report.warnings.push(format!(
            "backup {backup} not restored: the recorded backup is missing"
        ));
        return;
    }

    if sys.fs_is_symlink_no_err_async(dest).await
        || sys.fs_exists_no_err_async(dest).await
    {
        report.warnings.push(format!(
            "backup {backup} not restored: destination {} is still occupied",
            dest.display()
        ));
        return;
    }

    if sys.fs_rename_async(&backup_path, dest).await.is_err() {
        report.warnings.push(format!(
            "backup {backup} not restored: could not rename it back to {}",
            dest.display()
        ));
        return;
    }

    report.restored.push(dest.to_path_buf());
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
        // Follow the link so a deleted target counts as dangling: a broken
        // symlink still has an lstat entry an existence check would accept.
        let resolves = sys.fs_metadata_async(&dest).await.is_ok();

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
        let report = unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "nonexistent",
            BackupHandling::Leave,
        )
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

        let report = unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "id",
            BackupHandling::Leave,
        )
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

        let report = unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "id",
            BackupHandling::Leave,
        )
        .await
        .unwrap();

        assert_eq!(report.removed.len(), 1);
        assert!(!dest.exists());
        assert!(ledger.links().is_empty());
    }

    #[tokio::test]
    async fn prune_removes_symlink_with_deleted_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"data").unwrap();
        let dest = dir.path().join("dest.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &dest).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &dest).unwrap();
        // Deleting the target leaves a dangling symlink the ledger still tracks.
        std::fs::remove_file(&target).unwrap();

        let mut ledger = Ledger::from_links(vec![LedgerLink {
            source_id: "id".to_string(),
            dest: "dest.txt".to_string(),
            target: target.to_string_lossy().into_owned(),
            kind: ResolvedKind::Symlink,
            source_pin: "x".to_string(),
            backup: None,
        }]);

        let report = prune(&RealSys, dir.path(), &mut ledger).await.unwrap();

        assert_eq!(report.removed.len(), 1);
        assert!(ledger.links().is_empty());
        assert!(std::fs::symlink_metadata(&dest).is_err());
    }

    #[tokio::test]
    async fn prune_keeps_healthy_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"data").unwrap();
        let dest = dir.path().join("dest.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &dest).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &dest).unwrap();

        let mut ledger = Ledger::from_links(vec![LedgerLink {
            source_id: "id".to_string(),
            dest: "dest.txt".to_string(),
            target: target.to_string_lossy().into_owned(),
            kind: ResolvedKind::Symlink,
            source_pin: "x".to_string(),
            backup: None,
        }]);

        let report = prune(&RealSys, dir.path(), &mut ledger).await.unwrap();

        assert!(report.removed.is_empty());
        assert_eq!(ledger.links().len(), 1);
    }

    #[test]
    fn backup_handling_serde_round_trips_kebab_case() {
        for (value, wire) in [
            (BackupHandling::Leave, "\"leave\""),
            (BackupHandling::Clean, "\"clean\""),
            (BackupHandling::Restore, "\"restore\""),
        ] {
            let json = serde_json::to_string(&value).unwrap();
            assert_eq!(json, wire);
            let back: BackupHandling = serde_json::from_str(&json).unwrap();
            assert_eq!(value, back);
        }
    }

    #[test]
    fn backup_handling_defaults_to_leave() {
        assert_eq!(BackupHandling::default(), BackupHandling::Leave);
    }

    fn copy_link(source_id: &str, dest: &str, target: &Path) -> LedgerLink {
        LedgerLink {
            source_id: source_id.to_string(),
            dest: dest.to_string(),
            target: target.to_string_lossy().into_owned(),
            kind: ResolvedKind::Copy,
            source_pin: "x".to_string(),
            backup: None,
        }
    }

    #[tokio::test]
    async fn retain_sources_removes_orphan_and_keeps_configured() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"same").unwrap();

        let orphan = dir.path().join("orphan.txt");
        std::fs::write(&orphan, b"same").unwrap();
        let kept = dir.path().join("kept.txt");
        std::fs::write(&kept, b"same").unwrap();

        let mut ledger = Ledger::from_links(vec![
            copy_link("gone", "orphan.txt", &source),
            copy_link("present", "kept.txt", &source),
        ]);

        let keep_ids: HashSet<&str> = ["present"].into_iter().collect();
        let report =
            retain_sources(&RealSys, dir.path(), &mut ledger, &keep_ids)
                .await
                .unwrap();

        assert_eq!(report.removed.len(), 1);
        assert!(!orphan.exists(), "orphan dest removed");
        assert!(kept.exists(), "configured dest untouched");
        assert_eq!(ledger.links().len(), 1);
        assert_eq!(ledger.links()[0].source_id, "present");
    }

    #[tokio::test]
    async fn retain_sources_keeps_and_warns_modified_copy() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"original").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"user edited").unwrap();

        let mut ledger =
            Ledger::from_links(vec![copy_link("gone", "dest.txt", &source)]);

        let keep_ids: HashSet<&str> = HashSet::new();
        let report =
            retain_sources(&RealSys, dir.path(), &mut ledger, &keep_ids)
                .await
                .unwrap();

        assert!(report.removed.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(dest.exists(), "edited copy preserved");
        assert_eq!(ledger.links().len(), 1, "kept link stays recorded");
    }

    #[tokio::test]
    async fn retain_sources_prunes_empty_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"same").unwrap();
        let nested = dir.path().join("a/b/c/dest.txt");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, b"same").unwrap();

        let mut ledger = Ledger::from_links(vec![copy_link(
            "gone",
            "a/b/c/dest.txt",
            &source,
        )]);

        let keep_ids: HashSet<&str> = HashSet::new();
        retain_sources(&RealSys, dir.path(), &mut ledger, &keep_ids)
            .await
            .unwrap();

        assert!(!dir.path().join("a").exists(), "empty ancestors pruned");
    }

    #[tokio::test]
    async fn plan_retain_sources_reports_without_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"original").unwrap();
        let removable = dir.path().join("removable.txt");
        std::fs::write(&removable, b"original").unwrap();
        let edited = dir.path().join("edited.txt");
        std::fs::write(&edited, b"changed").unwrap();

        let ledger = Ledger::from_links(vec![
            copy_link("gone", "removable.txt", &source),
            copy_link("gone", "edited.txt", &source),
        ]);

        let keep_ids: HashSet<&str> = HashSet::new();
        let report =
            plan_retain_sources(&RealSys, dir.path(), &ledger, &keep_ids)
                .await
                .unwrap();

        assert_eq!(report.removed.len(), 1, "only the unmodified copy");
        assert_eq!(report.warnings.len(), 1, "edited copy reported as warning");
        assert!(removable.exists(), "dry-run touches nothing");
        assert!(edited.exists());
        assert_eq!(ledger.links().len(), 2, "ledger unchanged");
    }

    #[tokio::test]
    async fn unlink_clean_deletes_backup() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"same").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"same").unwrap();
        let backup = dir.path().join("dest.txt.bak.1");
        std::fs::write(&backup, b"pre-existing").unwrap();

        let mut link = copy_link("id", "dest.txt", &source);
        link.backup = Some("dest.txt.bak.1".to_string());
        let mut ledger = Ledger::from_links(vec![link]);

        unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "id",
            BackupHandling::Clean,
        )
        .await
        .unwrap();

        assert!(!dest.exists());
        assert!(!backup.exists(), "backup deleted");
    }

    #[tokio::test]
    async fn unlink_restore_renames_backup_back() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"same").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"same").unwrap();
        let backup = dir.path().join("dest.txt.bak.1");
        std::fs::write(&backup, b"pre-existing").unwrap();

        let mut link = copy_link("id", "dest.txt", &source);
        link.backup = Some("dest.txt.bak.1".to_string());
        let mut ledger = Ledger::from_links(vec![link]);

        let report = unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "id",
            BackupHandling::Restore,
        )
        .await
        .unwrap();

        assert_eq!(report.restored.len(), 1);
        assert!(!backup.exists(), "backup moved");
        assert!(dest.exists(), "backup restored to destination");
        assert_eq!(std::fs::read(&dest).unwrap(), b"pre-existing");
    }

    #[tokio::test]
    async fn unlink_restore_keeps_edited_copy_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"original").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"user edited").unwrap();
        let backup = dir.path().join("dest.txt.bak.1");
        std::fs::write(&backup, b"pre-existing").unwrap();

        let mut link = copy_link("id", "dest.txt", &source);
        link.backup = Some("dest.txt.bak.1".to_string());
        let mut ledger = Ledger::from_links(vec![link]);

        let report = unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "id",
            BackupHandling::Restore,
        )
        .await
        .unwrap();

        assert!(report.restored.is_empty());
        assert_eq!(report.warnings.len(), 2, "kept-copy and skipped-restore");
        assert_eq!(std::fs::read(&dest).unwrap(), b"user edited");
        assert!(backup.exists(), "backup left in place");
        assert_eq!(ledger.links().len(), 1);
    }

    #[tokio::test]
    async fn unlink_restore_missing_backup_warns_and_skips() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"same").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"same").unwrap();

        let mut link = copy_link("id", "dest.txt", &source);
        link.backup = Some("dest.txt.bak.1".to_string());
        let mut ledger = Ledger::from_links(vec![link]);

        let report = unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "id",
            BackupHandling::Restore,
        )
        .await
        .unwrap();

        assert_eq!(report.removed.len(), 1);
        assert!(report.restored.is_empty());
        assert_eq!(report.warnings.len(), 1, "missing backup warned");
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn unlink_leave_keeps_backup() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.txt");
        std::fs::write(&source, b"same").unwrap();
        let dest = dir.path().join("dest.txt");
        std::fs::write(&dest, b"same").unwrap();
        let backup = dir.path().join("dest.txt.bak.1");
        std::fs::write(&backup, b"pre-existing").unwrap();

        let mut link = copy_link("id", "dest.txt", &source);
        link.backup = Some("dest.txt.bak.1".to_string());
        let mut ledger = Ledger::from_links(vec![link]);

        unlink(
            &RealSys,
            dir.path(),
            &mut ledger,
            "id",
            BackupHandling::Leave,
        )
        .await
        .unwrap();

        assert!(!dest.exists());
        assert!(backup.exists(), "backup untouched under Leave");
    }
}
