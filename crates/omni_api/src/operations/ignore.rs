use std::path::{Path, PathBuf};

use omni_configurations::IgnoreConfig;
use omni_context::{Context, ContextSys};
use omni_ignore_contributors::{InternalContributor, ProjectionsContributor};
use omni_ignore_core::{
    CheckOutcome, CleanOutcome, Contribution, IgnoreContributor, SyncOutcome,
    check_files, clean_files, render_block, sync_files,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::operations::projection::ledger_path;

/// The system capabilities `omni ignore sync` needs: workspace context plus the
/// filesystem reads and writes the patch engine performs.
pub trait IgnoreSys: ContextSys {}
impl<T: ContextSys> IgnoreSys for T {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreSyncRequest {
    /// Print the block that would be written without touching any file.
    pub dry_run: bool,
    /// Report staleness without writing. `up_to_date` is false when any file is
    /// missing the block or carries an out-of-date one.
    pub check: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreCleanRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreFileChange {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreSyncResponse {
    pub dry_run: bool,
    pub check: bool,
    /// The rendered managed block, for display under `--dry-run`.
    pub block: String,
    pub files: Vec<IgnoreFileChange>,
    /// True when every configured file already carries the current block.
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IgnoreCleanResponse {
    pub files: Vec<IgnoreFileChange>,
}

fn resolve_files(root: &Path, cfg: &IgnoreConfig) -> Vec<PathBuf> {
    let names: Vec<&str> = if cfg.files.is_empty() {
        omni_constants::DEFAULT_IGNORE_FILES.to_vec()
    } else {
        cfg.files.iter().map(String::as_str).collect()
    };
    names.iter().map(|name| root.join(name)).collect()
}

async fn gather_contributions<TSys: IgnoreSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
) -> eyre::Result<Vec<Contribution>> {
    let ignore = &ctx.workspace_configuration().ignore;
    let mut contributors: Vec<Box<dyn IgnoreContributor>> = Vec::new();
    if ignore.sources.internal {
        contributors.push(Box::new(InternalContributor));
    }
    if ignore.sources.projections {
        contributors.push(Box::new(ProjectionsContributor::new(
            sys.clone(),
            ledger_path(ctx),
        )));
    }

    let mut contributions = Vec::with_capacity(contributors.len());
    for contributor in &contributors {
        let patterns = contributor.patterns().await?;
        contributions.push(Contribution {
            name: contributor.name(),
            patterns,
        });
    }
    Ok(contributions)
}

fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Patch (or, under `dry_run`/`check`, only inspect) the managed block in every
/// configured ignore file.
pub async fn handle_ignore_sync<TSys: IgnoreSys>(
    ctx: &Context<TSys>,
    req: IgnoreSyncRequest,
) -> eyre::Result<IgnoreSyncResponse> {
    let sys = ctx.sys().clone();
    let root = ctx.root_dir().to_path_buf();
    let cfg = ctx.workspace_configuration().ignore.clone();
    let files = resolve_files(&root, &cfg);
    let contributions = gather_contributions(ctx, &sys).await?;
    let block = render_block(&contributions);

    if req.check || req.dry_run {
        let reports = check_files(&sys, &files, &block).await?;
        let up_to_date = reports
            .iter()
            .all(|report| report.outcome == CheckOutcome::UpToDate);
        let files = reports
            .into_iter()
            .map(|report| IgnoreFileChange {
                path: display(&report.path, &root),
                status: check_status(&report.outcome).to_string(),
            })
            .collect();
        return Ok(IgnoreSyncResponse {
            dry_run: req.dry_run,
            check: req.check,
            block,
            files,
            up_to_date,
        });
    }

    let reports = sync_files(&sys, &files, &block).await?;
    let files = reports
        .into_iter()
        .map(|report| IgnoreFileChange {
            path: display(&report.path, &root),
            status: sync_status(&report.outcome).to_string(),
        })
        .collect();
    Ok(IgnoreSyncResponse {
        dry_run: false,
        check: false,
        block,
        files,
        up_to_date: true,
    })
}

/// Remove the managed block from every configured ignore file.
pub async fn handle_ignore_clean<TSys: IgnoreSys>(
    ctx: &Context<TSys>,
    _req: IgnoreCleanRequest,
) -> eyre::Result<IgnoreCleanResponse> {
    let sys = ctx.sys().clone();
    let root = ctx.root_dir().to_path_buf();
    let cfg = ctx.workspace_configuration().ignore.clone();
    let files = resolve_files(&root, &cfg);

    let reports = clean_files(&sys, &files).await?;
    let files = reports
        .into_iter()
        .map(|report| IgnoreFileChange {
            path: display(&report.path, &root),
            status: clean_status(&report.outcome).to_string(),
        })
        .collect();
    Ok(IgnoreCleanResponse { files })
}

fn sync_status(outcome: &SyncOutcome) -> &'static str {
    match outcome {
        SyncOutcome::Created => "created",
        SyncOutcome::Updated => "updated",
        SyncOutcome::Unchanged => "unchanged",
    }
}

fn check_status(outcome: &CheckOutcome) -> &'static str {
    match outcome {
        CheckOutcome::UpToDate => "up-to-date",
        CheckOutcome::Missing => "missing",
        CheckOutcome::Stale => "stale",
    }
}

fn clean_status(outcome: &CleanOutcome) -> &'static str {
    match outcome {
        CleanOutcome::Removed => "removed",
        CleanOutcome::Absent => "absent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_files_resolve_to_the_three_root_defaults() {
        let cfg = IgnoreConfig::default();
        let files = resolve_files(Path::new("/ws"), &cfg);
        assert_eq!(
            files,
            vec![
                PathBuf::from("/ws/.gitignore"),
                PathBuf::from("/ws/.ignore"),
                PathBuf::from("/ws/.omniignore"),
            ]
        );
    }

    #[test]
    fn an_explicit_files_list_is_honored_verbatim() {
        let cfg = IgnoreConfig {
            files: vec![
                ".gitignore".to_string(),
                "nested/.gitignore".to_string(),
            ],
            ..Default::default()
        };
        let files = resolve_files(Path::new("/ws"), &cfg);
        assert_eq!(
            files,
            vec![
                PathBuf::from("/ws/.gitignore"),
                PathBuf::from("/ws/nested/.gitignore"),
            ]
        );
    }

    #[test]
    fn display_is_relative_to_the_workspace_root() {
        assert_eq!(
            display(Path::new("/ws/.gitignore"), Path::new("/ws")),
            ".gitignore"
        );
    }
}
