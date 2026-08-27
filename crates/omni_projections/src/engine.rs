use std::path::{Path, PathBuf};

use omni_projection_configurations::Projection;
use system_traits::{FsCanonicalizeAsync, FsMetadataAsync, FsReadAsync};

use crate::apply::{
    ApplierSys, ApplyOptions, ApplyOutcome, PriorLink, ResolvedKind, apply_link,
};
use crate::containment::source_realpath_contained;
use crate::error::{ProjectionError, Result};
use crate::ledger::{
    Ledger, LedgerLink, copy_was_modified, local_source_pin,
    prune_empty_ancestors, remove_path,
};
use crate::routing::{LinkPair, PlanInput, ScannedEntry, plan};
use crate::scan::scan_source;

/// A projection source resolved to a concrete root directory on disk.
pub struct ResolvedSource<'a> {
    pub id: &'a str,
    pub source_root: &'a Path,
    /// The git commit pin when the source is a repository; `None` for local
    /// sources, for which a content hash over matched files is computed.
    pub git_pin: Option<String>,
    pub projections: &'a [Projection],
}

/// Cross-cutting inputs for one sync pass.
pub struct SyncParams<'a> {
    pub workspace_root: &'a Path,
    pub env_files: &'a [String],
    /// Re-apply and repair even when the pin is unchanged.
    pub force: bool,
    /// Compute the full plan without touching the filesystem.
    pub dry_run: bool,
}

/// A link the planner intends to materialize (surfaced for dry-run output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedLink {
    pub source_id: String,
    pub dest: String,
    pub source_abs: PathBuf,
}

/// The result of materializing (or skipping) one planned link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedLink {
    pub dest: String,
    pub kind: ResolvedKind,
    pub backup: Option<String>,
    pub skipped: bool,
}

/// Everything one source's sync produced.
#[derive(Debug, Clone, Default)]
pub struct SyncSourceOutcome {
    /// Desired ledger links for this source after the pass.
    pub links: Vec<LedgerLink>,
    pub planned: Vec<PlannedLink>,
    pub applied: Vec<AppliedLink>,
    /// Stale destinations reconciled away (no longer desired).
    pub removed: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Plan and apply every projection for one resolved source, reconciling away
/// any stale destinations previously recorded for it. Pure with respect to the
/// network: the caller resolves git/local roots first.
pub async fn sync_source<S>(
    sys: &S,
    source: &ResolvedSource<'_>,
    params: &SyncParams<'_>,
    prior_ledger: &Ledger,
) -> Result<SyncSourceOutcome>
where
    S: ApplierSys + FsReadAsync + FsCanonicalizeAsync,
{
    let entries = scan_source(sys, source.source_root).await?;
    let mut outcome = SyncSourceOutcome::default();
    let mut desired_dests: Vec<String> = Vec::new();

    for projection in source.projections {
        let pairs = plan(&PlanInput {
            workspace_root: params.workspace_root,
            source_root: source.source_root,
            source_id: source.id,
            projection,
            entries: &entries,
            env_files: params.env_files,
        })?;

        let pin = match &source.git_pin {
            Some(commit) => commit.clone(),
            None => {
                let matched =
                    matched_files(&pairs, source.source_root, &entries);
                local_source_pin(sys, source.source_root, &matched).await?
            }
        };

        let opts = ApplyOptions {
            link: projection.link,
            collision: projection.on_collision,
        };

        for pair in &pairs {
            if !source_realpath_contained(
                sys,
                source.source_root,
                &pair.source_abs,
            )
            .await?
            {
                return Err(ProjectionError::custom(format!(
                    "source escapes the source root: {}",
                    pair.source_abs.display()
                )));
            }

            let dest_rel = rel_string(params.workspace_root, &pair.dest_abs)?;
            desired_dests.push(dest_rel.clone());

            outcome.planned.push(PlannedLink {
                source_id: source.id.to_string(),
                dest: dest_rel.clone(),
                source_abs: pair.source_abs.clone(),
            });

            if params.dry_run {
                continue;
            }

            let prior = prior_ledger
                .links()
                .iter()
                .find(|l| l.source_id == source.id && l.dest == dest_rel);
            let prior_link = prior.map(|l| PriorLink {
                source_abs: Path::new(&l.target),
                pin_unchanged: !params.force && l.source_pin == pin,
            });

            let apply_outcome =
                apply_link(sys, pair, &opts, prior_link.as_ref()).await?;

            let (kind, backup) = match &apply_outcome {
                ApplyOutcome::Linked { kind, backup } => (
                    *kind,
                    backup
                        .as_ref()
                        .map(|b| rel_string_lossy(params.workspace_root, b)),
                ),
                ApplyOutcome::Skipped => (
                    prior.map(|l| l.kind).unwrap_or(ResolvedKind::Symlink),
                    prior.and_then(|l| l.backup.clone()),
                ),
            };

            outcome.applied.push(AppliedLink {
                dest: dest_rel.clone(),
                kind,
                backup: backup.clone(),
                skipped: matches!(apply_outcome, ApplyOutcome::Skipped),
            });

            outcome.links.push(LedgerLink {
                source_id: source.id.to_string(),
                dest: dest_rel,
                target: pair.source_abs.to_string_lossy().into_owned(),
                kind,
                source_pin: pin.clone(),
                backup,
            });
        }
    }

    if !params.dry_run {
        reconcile_stale(
            sys,
            source,
            params,
            prior_ledger,
            &desired_dests,
            &mut outcome,
        )
        .await?;
    }

    Ok(outcome)
}

/// Remove destinations recorded for this source in a prior pass that the
/// current config no longer wants. User-edited copies are kept and warned on.
async fn reconcile_stale<S>(
    sys: &S,
    source: &ResolvedSource<'_>,
    params: &SyncParams<'_>,
    prior_ledger: &Ledger,
    desired_dests: &[String],
    outcome: &mut SyncSourceOutcome,
) -> Result<()>
where
    S: ApplierSys + FsReadAsync,
{
    for link in prior_ledger
        .links()
        .iter()
        .filter(|l| l.source_id == source.id)
    {
        if desired_dests.contains(&link.dest) {
            continue;
        }

        let dest_abs = params.workspace_root.join(&link.dest);

        if link.kind == ResolvedKind::Copy
            && copy_was_modified(sys, &dest_abs, &link.target).await
        {
            outcome.warnings.push(format!(
                "kept modified copy at {} (edited since projection)",
                dest_abs.display()
            ));
            outcome.links.push(link.clone());
            continue;
        }

        if remove_path(sys, &dest_abs).await? {
            prune_empty_ancestors(sys, params.workspace_root, &dest_abs).await;
            outcome.removed.push(dest_abs);
        }
    }

    Ok(())
}

/// The classified state of a recorded link, reported by [`status`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// The destination exists and resolves.
    Ok,
    /// The destination is absent.
    Missing,
    /// A symlink whose target no longer resolves.
    Broken,
    /// A path exists where a symlink was recorded, but it is not a symlink.
    Drifted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub source_id: String,
    pub dest: String,
    pub state: LinkState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusReport {
    pub entries: Vec<StatusEntry>,
}

impl StatusReport {
    /// Whether any recorded link is in a non-`Ok` state.
    pub fn has_problems(&self) -> bool {
        self.entries.iter().any(|e| e.state != LinkState::Ok)
    }
}

/// Classify every recorded link against the current filesystem. Read-only and
/// network-free: it inspects only the destinations the ledger already tracks.
pub async fn status<S>(
    sys: &S,
    workspace_root: &Path,
    ledger: &Ledger,
) -> Result<StatusReport>
where
    S: FsMetadataAsync + Sync,
{
    let mut report = StatusReport::default();

    for link in ledger.links() {
        let dest = workspace_root.join(&link.dest);
        let is_symlink = sys.fs_is_symlink_no_err_async(&dest).await;
        let resolves = sys.fs_exists_no_err_async(&dest).await;

        let state = match link.kind {
            ResolvedKind::Copy | ResolvedKind::Hardlink => {
                if resolves {
                    LinkState::Ok
                } else {
                    LinkState::Missing
                }
            }
            ResolvedKind::Symlink | ResolvedKind::Junction => {
                if is_symlink {
                    if resolves {
                        LinkState::Ok
                    } else {
                        LinkState::Broken
                    }
                } else if resolves {
                    LinkState::Drifted
                } else {
                    LinkState::Missing
                }
            }
        };

        report.entries.push(StatusEntry {
            source_id: link.source_id.clone(),
            dest: link.dest.clone(),
            state,
        });
    }

    Ok(report)
}

fn matched_files(
    pairs: &[LinkPair],
    source_root: &Path,
    entries: &[ScannedEntry],
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for pair in pairs {
        match pair.source_abs.strip_prefix(source_root) {
            // A whole-source (namespaced) link: hash every file in the tree.
            Ok(rel) if rel.as_os_str().is_empty() => {
                out.extend(
                    entries.iter().filter(|e| !e.is_dir).map(|e| e.rel.clone()),
                );
            }
            Ok(rel) => out.push(rel.to_path_buf()),
            Err(_) => {}
        }
    }
    out
}

fn rel_string(workspace_root: &Path, path: &Path) -> Result<String> {
    path.strip_prefix(workspace_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|_| {
            ProjectionError::custom(format!(
                "destination is outside the workspace: {}",
                path.display()
            ))
        })
}

fn rel_string_lossy(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_projection_configurations::Projection;
    use system_traits::impls::RealSys;

    fn projection(json: &str) -> Projection {
        serde_json::from_str(json).expect("valid projection")
    }

    #[tokio::test]
    async fn sync_creates_links_and_records_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let src_root = ws.join("vendor/pkg");
        std::fs::create_dir_all(&src_root).unwrap();
        std::fs::write(src_root.join("a.txt"), b"a").unwrap();

        let projections = vec![projection(
            r#"{"strategy":"mirror","target":"@workspace/dst"}"#,
        )];
        let source = ResolvedSource {
            id: "pkg",
            source_root: &src_root,
            git_pin: None,
            projections: &projections,
        };
        let params = SyncParams {
            workspace_root: ws,
            env_files: &[],
            force: false,
            dry_run: false,
        };

        let outcome =
            sync_source(&RealSys, &source, &params, &Ledger::default())
                .await
                .unwrap();

        assert_eq!(outcome.links.len(), 1);
        assert_eq!(outcome.links[0].dest, "dst/a.txt");
        assert!(ws.join("dst/a.txt").exists());
    }

    #[tokio::test]
    async fn dry_run_plans_without_touching_disk() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let src_root = ws.join("vendor/pkg");
        std::fs::create_dir_all(&src_root).unwrap();
        std::fs::write(src_root.join("a.txt"), b"a").unwrap();

        let projections = vec![projection(
            r#"{"strategy":"mirror","target":"@workspace/dst"}"#,
        )];
        let source = ResolvedSource {
            id: "pkg",
            source_root: &src_root,
            git_pin: None,
            projections: &projections,
        };
        let params = SyncParams {
            workspace_root: ws,
            env_files: &[],
            force: false,
            dry_run: true,
        };

        let outcome =
            sync_source(&RealSys, &source, &params, &Ledger::default())
                .await
                .unwrap();

        assert_eq!(outcome.planned.len(), 1);
        assert!(outcome.links.is_empty());
        assert!(outcome.applied.is_empty());
        assert!(!ws.join("dst/a.txt").exists());
    }

    #[tokio::test]
    async fn reconcile_removes_stale_links() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let src_root = ws.join("vendor/pkg");
        std::fs::create_dir_all(&src_root).unwrap();
        std::fs::write(src_root.join("a.txt"), b"a").unwrap();

        let projections = vec![projection(
            r#"{"strategy":"mirror","target":"@workspace/dst"}"#,
        )];
        let source = ResolvedSource {
            id: "pkg",
            source_root: &src_root,
            git_pin: None,
            projections: &projections,
        };
        let params = SyncParams {
            workspace_root: ws,
            env_files: &[],
            force: false,
            dry_run: false,
        };

        // A prior ledger recorded a now-unwanted symlink.
        std::fs::create_dir_all(ws.join("dst")).unwrap();
        let stale = ws.join("dst/old.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(src_root.join("a.txt"), &stale).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(src_root.join("a.txt"), &stale)
            .unwrap();

        let prior = Ledger::from_links(vec![LedgerLink {
            source_id: "pkg".to_string(),
            dest: "dst/old.txt".to_string(),
            target: src_root.join("a.txt").to_string_lossy().into_owned(),
            kind: ResolvedKind::Symlink,
            source_pin: "old".to_string(),
            backup: None,
        }]);

        let outcome = sync_source(&RealSys, &source, &params, &prior)
            .await
            .unwrap();

        assert!(!stale.exists(), "stale link must be reconciled away");
        assert!(ws.join("dst/a.txt").exists());
        assert!(outcome.links.iter().all(|l| l.dest != "dst/old.txt"));
    }

    #[tokio::test]
    async fn status_flags_broken_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        std::fs::create_dir_all(ws.join("dst")).unwrap();

        let good = ws.join("dst/good.txt");
        std::fs::write(ws.join("dst/target.txt"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(ws.join("dst/target.txt"), &good).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(ws.join("dst/target.txt"), &good)
            .unwrap();

        let ledger = Ledger::from_links(vec![
            LedgerLink {
                source_id: "s".to_string(),
                dest: "dst/good.txt".to_string(),
                target: "dst/target.txt".to_string(),
                kind: ResolvedKind::Symlink,
                source_pin: "p".to_string(),
                backup: None,
            },
            LedgerLink {
                source_id: "s".to_string(),
                dest: "dst/missing.txt".to_string(),
                target: "somewhere".to_string(),
                kind: ResolvedKind::Symlink,
                source_pin: "p".to_string(),
                backup: None,
            },
        ]);

        let report = status(&RealSys, ws, &ledger).await.unwrap();
        assert!(report.has_problems());
        let missing = report
            .entries
            .iter()
            .find(|e| e.dest == "dst/missing.txt")
            .unwrap();
        assert_eq!(missing.state, LinkState::Missing);
        let good = report
            .entries
            .iter()
            .find(|e| e.dest == "dst/good.txt")
            .unwrap();
        assert_eq!(good.state, LinkState::Ok);
    }
}
