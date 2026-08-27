use omni_configuration_discovery::ConfigurationDiscovery;
use omni_configurations::{SourceConfig, types::SingleOrMany};
use omni_context::{Context, ContextSys};
use omni_projection_configurations::{
    OwnedProjectionConfiguration, Projection, ProjectionExtra,
};
use omni_projections::{
    ApplierSys, LinkState, ResolvedSource, SyncParams, sync_source,
};
use omni_remote_sources::{
    manager::{RemoteSourceManager, config::RemoteSourceConfig},
    sys::RemoteSourceSys,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use system_traits::{FsCanonicalizeAsync, FsReadAsync};
use url::Url;

// ── Request / response types ────────────────────────────────────────────────

/// Parameters for [`handle_projection_sync`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionSyncRequest {
    /// Compute the full plan without touching the filesystem.
    pub dry_run: bool,
    /// Re-apply and repair every link even when its pin is unchanged.
    pub force: bool,
    /// Re-resolve mutable git revisions (e.g. branches) before applying.
    pub update: bool,
    /// Limit the pass to the projection source with this `id`.
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlannedLinkInfo {
    pub source_id: String,
    pub dest: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AppliedLinkInfo {
    pub dest: String,
    pub kind: String,
    pub backup: Option<String>,
    pub skipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionSyncResponse {
    pub dry_run: bool,
    pub planned: Vec<PlannedLinkInfo>,
    pub applied: Vec<AppliedLinkInfo>,
    pub removed: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionStatusRequest {
    pub verbose: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusEntryInfo {
    pub source_id: String,
    pub dest: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionStatusResponse {
    pub entries: Vec<StatusEntryInfo>,
    pub ok: usize,
    pub missing: usize,
    pub broken: usize,
    pub drifted: usize,
    pub has_problems: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionUnlinkRequest {
    pub id: String,
    pub clean_backups: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionUnlinkResponse {
    pub removed: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionPruneRequest {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectionPruneResponse {
    pub dry_run: bool,
    pub removed: Vec<String>,
}

// ── The system surface a projection pass needs ──────────────────────────────

/// Everything a projection sync/teardown pass requires from a system handle.
pub trait ProjectionSys:
    ContextSys + RemoteSourceSys + ApplierSys + FsReadAsync + FsCanonicalizeAsync
{
}
impl<T> ProjectionSys for T where
    T: ContextSys
        + RemoteSourceSys
        + ApplierSys
        + FsReadAsync
        + FsCanonicalizeAsync
{
}

// ── Operations ──────────────────────────────────────────────────────────────

/// Resolve every configured projection source, plan and apply its links, and
/// persist the ledger. `resolve -> plan -> apply -> ledger -> retain -> lock`.
pub async fn handle_projection_sync<TSys>(
    ctx: &Context<TSys>,
    req: ProjectionSyncRequest,
) -> eyre::Result<ProjectionSyncResponse>
where
    TSys: ProjectionSys,
{
    let sys = ctx.sys().clone();
    let workspace_root = ctx.root_dir().to_path_buf();
    let env_files = env_file_names(ctx);
    let sources = &ctx.workspace_configuration().projections;

    let remote = build_remote_manager(ctx, &sys).await?;

    let ledger_path = ledger_path(ctx);
    let mut ledger = omni_projections::load(&sys, &ledger_path).await;
    let prior_ledger = ledger.clone();

    // Retain every configured git source so an unrelated, unfiltered run never
    // garbage-collects a source it simply did not touch.
    let all_git: Vec<(Url, String)> = sources
        .iter()
        .filter_map(|s| match s {
            SourceConfig::Git(g) => Some((g.uri.clone(), g.rev.clone())),
            SourceConfig::Local(_) => None,
        })
        .collect();

    let mut response = ProjectionSyncResponse {
        dry_run: req.dry_run,
        planned: Vec::new(),
        applied: Vec::new(),
        removed: Vec::new(),
        warnings: Vec::new(),
    };

    for source in sources {
        let id = source_id(source);

        if let Some(filter) = &req.source {
            if filter != id {
                continue;
            }
        }

        // An explicit empty `routes` list projects nothing: pure workspace
        // config, caught before any source is materialized.
        if matches!(workspace_routes(source), Some(routes) if routes.is_empty())
        {
            return Err(eyre::eyre!(
                "projection source '{id}' declares an empty `routes` list; a projection source must project at least one route"
            ));
        }

        let (source_root, git_pin) = match source {
            SourceConfig::Local(local) => {
                let path = single_path(id, &local.path)?;
                let root = path_clean::clean(workspace_root.join(path));
                (root, None)
            }
            SourceConfig::Git(git) => {
                if req.update {
                    remote.invalidate_git(&git.uri, &git.rev).await?;
                }
                let dir = remote.pull_git_repo(&git.uri, &git.rev).await?;
                let pin = remote.locked_commit(&git.uri, &git.rev).await;
                (dir, pin)
            }
        };

        let effective_routes = resolve_effective_routes(
            &sys,
            id,
            workspace_routes(source),
            &source_root,
        )
        .await?;

        let resolved = ResolvedSource {
            id,
            source_root: &source_root,
            git_pin,
            projections: &effective_routes,
        };
        let params = SyncParams {
            workspace_root: &workspace_root,
            env_files: &env_files,
            force: req.force,
            dry_run: req.dry_run,
        };

        let outcome =
            sync_source(&sys, &resolved, &params, &prior_ledger).await?;

        response.planned.extend(outcome.planned.iter().map(|p| {
            PlannedLinkInfo {
                source_id: p.source_id.clone(),
                dest: p.dest.clone(),
                target: p.source_abs.to_string_lossy().into_owned(),
            }
        }));
        response.applied.extend(outcome.applied.iter().map(|a| {
            AppliedLinkInfo {
                dest: a.dest.clone(),
                kind: kind_str(a.kind).to_string(),
                backup: a.backup.clone(),
                skipped: a.skipped,
            }
        }));
        response.removed.extend(
            outcome
                .removed
                .iter()
                .map(|p| p.to_string_lossy().into_owned()),
        );
        response.warnings.extend(outcome.warnings.iter().cloned());

        if !req.dry_run {
            ledger.links_mut().retain(|l| l.source_id != *id);
            ledger.links_mut().extend(outcome.links);
        }
    }

    if !req.dry_run {
        omni_projections::save(&sys, &ledger_path, &ledger).await?;
        let refs: Vec<(&Url, &str)> =
            all_git.iter().map(|(u, r)| (u, r.as_str())).collect();
        remote.retain_git_sources(&refs).await?;
        remote.lock().await?;
    }

    Ok(response)
}

/// Classify every recorded link against the current filesystem. Read-only.
pub async fn handle_projection_status<TSys>(
    ctx: &Context<TSys>,
    _req: ProjectionStatusRequest,
) -> eyre::Result<ProjectionStatusResponse>
where
    TSys: ContextSys,
{
    let sys = ctx.sys().clone();
    let workspace_root = ctx.root_dir().to_path_buf();

    let ledger = omni_projections::load(&sys, &ledger_path(ctx)).await;
    let report =
        omni_projections::status(&sys, &workspace_root, &ledger).await?;

    let mut resp = ProjectionStatusResponse {
        entries: Vec::new(),
        ok: 0,
        missing: 0,
        broken: 0,
        drifted: 0,
        has_problems: report.has_problems(),
    };

    for entry in &report.entries {
        match entry.state {
            LinkState::Ok => resp.ok += 1,
            LinkState::Missing => resp.missing += 1,
            LinkState::Broken => resp.broken += 1,
            LinkState::Drifted => resp.drifted += 1,
        }
        resp.entries.push(StatusEntryInfo {
            source_id: entry.source_id.clone(),
            dest: entry.dest.clone(),
            state: state_str(entry.state).to_string(),
        });
    }

    Ok(resp)
}

/// Remove only the ledger-recorded links for one source `id`.
pub async fn handle_projection_unlink<TSys>(
    ctx: &Context<TSys>,
    req: ProjectionUnlinkRequest,
) -> eyre::Result<ProjectionUnlinkResponse>
where
    TSys: ContextSys + ApplierSys + FsReadAsync,
{
    let sys = ctx.sys().clone();
    let workspace_root = ctx.root_dir().to_path_buf();

    let ledger_path = ledger_path(ctx);
    let mut ledger = omni_projections::load(&sys, &ledger_path).await;
    let report = omni_projections::unlink(
        &sys,
        &workspace_root,
        &mut ledger,
        &req.id,
        req.clean_backups,
    )
    .await?;
    omni_projections::save(&sys, &ledger_path, &ledger).await?;

    Ok(ProjectionUnlinkResponse {
        removed: report
            .removed
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        warnings: report.warnings,
    })
}

/// Remove ledger-recorded links whose destination has become a dangling
/// symlink. Never touches unrecorded files.
pub async fn handle_projection_prune<TSys>(
    ctx: &Context<TSys>,
    req: ProjectionPruneRequest,
) -> eyre::Result<ProjectionPruneResponse>
where
    TSys: ContextSys + ApplierSys,
{
    let sys = ctx.sys().clone();
    let workspace_root = ctx.root_dir().to_path_buf();

    let ledger_path = ledger_path(ctx);
    let mut ledger = omni_projections::load(&sys, &ledger_path).await;

    if req.dry_run {
        // A prune preview classifies broken links without removing them.
        let report =
            omni_projections::status(&sys, &workspace_root, &ledger).await?;
        let removed = report
            .entries
            .iter()
            .filter(|e| e.state == LinkState::Broken)
            .map(|e| {
                workspace_root.join(&e.dest).to_string_lossy().into_owned()
            })
            .collect();
        return Ok(ProjectionPruneResponse {
            dry_run: true,
            removed,
        });
    }

    let report =
        omni_projections::prune(&sys, &workspace_root, &mut ledger).await?;
    omni_projections::save(&sys, &ledger_path, &ledger).await?;

    Ok(ProjectionPruneResponse {
        dry_run: false,
        removed: report
            .removed
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

async fn build_remote_manager<TSys>(
    ctx: &Context<TSys>,
    sys: &TSys,
) -> eyre::Result<RemoteSourceManager<TSys>>
where
    TSys: ContextSys + RemoteSourceSys,
{
    let sources_path = projection_sources_dir(ctx);
    sys.fs_create_dir_all_async(&sources_path).await?;
    let lockfile_path = sources_path.join("lock.json");

    Ok(RemoteSourceManager::new(
        RemoteSourceConfig::builder()
            .lockfile_path(lockfile_path)
            .soure_dir_path(sources_path)
            .build(),
        sys.clone(),
    )
    .await?)
}

/// The directory holding all projection-source state (git checkouts, lockfile,
/// and the link ledger), alongside the other subsystem source caches.
fn projection_sources_dir<TSys: ContextSys>(
    ctx: &Context<TSys>,
) -> std::path::PathBuf {
    ctx.omni_dir().join("sources/projection")
}

/// The link ledger location. Owned by this layer, not the projection engine.
fn ledger_path<TSys: ContextSys>(ctx: &Context<TSys>) -> std::path::PathBuf {
    projection_sources_dir(ctx).join("links.json")
}

fn source_id(source: &SourceConfig<ProjectionExtra>) -> &str {
    match source {
        SourceConfig::Local(l) => l.extra.id.as_str(),
        SourceConfig::Git(g) => g.extra.id.as_str(),
    }
}

fn workspace_routes(
    source: &SourceConfig<ProjectionExtra>,
) -> Option<&[Projection]> {
    let extra = match source {
        SourceConfig::Local(l) => &l.extra,
        SourceConfig::Git(g) => &g.extra,
    };
    extra.routes.as_deref()
}

/// Resolve the effective routes for one source. Workspace routes override the
/// source's owned manifest wholesale; an absent workspace list inherits it.
async fn resolve_effective_routes<TSys>(
    sys: &TSys,
    id: &str,
    workspace_routes: Option<&[Projection]>,
    source_root: &std::path::Path,
) -> eyre::Result<Vec<Projection>>
where
    TSys: FsReadAsync + Send + Sync + Clone,
{
    match workspace_routes {
        // Non-empty (the empty case is rejected before materialization).
        Some(routes) => {
            if discover_owned_manifest(sys, source_root).await?.is_some() {
                log::debug!(
                    "projection source '{id}': workspace `routes` override the source's projection.omni manifest"
                );
            }
            Ok(routes.to_vec())
        }
        None => match discover_owned_manifest(sys, source_root).await? {
            Some(owned) if !owned.routes.is_empty() => {
                reject_privilege_escalation(id, &owned.routes)?;
                Ok(owned.routes)
            }
            _ => Err(eyre::eyre!(
                "projection source '{id}' declares no routes and its source ships no projection.omni.yaml"
            )),
        },
    }
}

/// Discover a `projection.omni.{yaml,yml,json,toml}` at the source root, honoring
/// `.omniignore`. Mirrors `omni_generator::discover_one_in_dir`.
async fn discover_owned_manifest<TSys>(
    sys: &TSys,
    source_root: &std::path::Path,
) -> eyre::Result<Option<OwnedProjectionConfiguration>>
where
    TSys: FsReadAsync + Send + Sync,
{
    // The candidate names for a source-owned projection manifest, computed once
    // from the shared config extensions.
    static NAMES: std::sync::LazyLock<Vec<String>> =
        std::sync::LazyLock::new(|| {
            omni_constants::config_file_names(omni_constants::PROJECTION_OMNI)
        });
    const IGNORE_FILES: [&str; 1] = [omni_constants::OMNI_IGNORE];

    let discovery = ConfigurationDiscovery::new(
        source_root,
        &NAMES[..],
        &NAMES[..],
        &IGNORE_FILES[..],
        "projection",
    );

    for file in discovery.discover().await? {
        let owned: OwnedProjectionConfiguration =
            omni_file_data_serde::read_async(file.as_path(), sys).await?;
        return Ok(Some(owned));
    }

    Ok(None)
}

/// The `allow_omni_config`/`allow_git` escape hatches are honored only when set
/// in the workspace configuration; a source-declared route setting either is
/// rejected so a source cannot grant itself control-plane access.
fn reject_privilege_escalation(
    id: &str,
    routes: &[Projection],
) -> eyre::Result<()> {
    for route in routes {
        let common = route.common();
        if common.allow_omni_config || common.allow_git {
            return Err(eyre::eyre!(
                "owned projection from source '{id}' sets `allow_omni_config`/`allow_git`; those are honored only in workspace configuration"
            ));
        }
    }
    Ok(())
}

fn single_path<'a>(
    id: &str,
    path: &'a SingleOrMany<String>,
) -> eyre::Result<&'a str> {
    match path {
        SingleOrMany::Single(p) => Ok(p.as_str()),
        SingleOrMany::Many(items) if items.len() == 1 => Ok(items[0].as_str()),
        SingleOrMany::Many(_) => Err(eyre::eyre!(
            "projection source '{id}' must specify a single `path` (its source root)"
        )),
    }
}

fn env_file_names<TSys: ContextSys>(ctx: &Context<TSys>) -> Vec<String> {
    ctx.env_files()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

fn kind_str(kind: omni_projections::ResolvedKind) -> &'static str {
    use omni_projections::ResolvedKind::*;
    match kind {
        Symlink => "symlink",
        Junction => "junction",
        Hardlink => "hardlink",
        Copy => "copy",
    }
}

fn state_str(state: LinkState) -> &'static str {
    match state {
        LinkState::Ok => "ok",
        LinkState::Missing => "missing",
        LinkState::Broken => "broken",
        LinkState::Drifted => "drifted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger_link(
        source_id: &str,
        dest: &str,
        target: &str,
    ) -> omni_projections::LedgerLink {
        omni_projections::LedgerLink {
            source_id: source_id.to_string(),
            dest: dest.to_string(),
            target: target.to_string(),
            kind: omni_projections::ResolvedKind::Symlink,
            source_pin: "p".to_string(),
            backup: None,
        }
    }

    #[tokio::test]
    async fn status_reports_are_deterministic() {
        use system_traits::impls::RealSys;

        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let ledger = omni_projections::Ledger::from_links(vec![ledger_link(
            "s",
            "missing.txt",
            "somewhere",
        )]);
        omni_projections::save(&RealSys, &ws.join("links.json"), &ledger)
            .await
            .unwrap();

        let report = omni_projections::status(&RealSys, ws, &ledger)
            .await
            .unwrap();
        assert!(report.has_problems());
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].state, LinkState::Missing);
    }

    #[test]
    fn single_path_rejects_multiple() {
        let many = SingleOrMany::Many(vec!["a".into(), "b".into()]);
        assert!(single_path("id", &many).is_err());
        let single = SingleOrMany::Single("a".into());
        assert_eq!(single_path("id", &single).unwrap(), "a");
    }

    fn route(json: &str) -> Projection {
        serde_json::from_str(json).expect("valid route")
    }

    #[tokio::test]
    async fn owned_manifest_is_inherited_when_workspace_omits_routes() {
        use system_traits::impls::RealSys;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("projection.omni.yaml"),
            "routes:\n  - strategy: namespaced\n    target: \"@workspace/vendored\"\n",
        )
        .unwrap();

        let routes = resolve_effective_routes(&RealSys, "id", None, dir.path())
            .await
            .unwrap();
        assert_eq!(routes.len(), 1);
        assert!(matches!(routes[0], Projection::Namespaced(_)));
    }

    #[tokio::test]
    async fn workspace_routes_override_owned_manifest() {
        use system_traits::impls::RealSys;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("projection.omni.yaml"),
            "routes:\n  - strategy: namespaced\n",
        )
        .unwrap();

        let ws = vec![route(r#"{"strategy":"mirror"}"#)];
        let routes =
            resolve_effective_routes(&RealSys, "id", Some(&ws), dir.path())
                .await
                .unwrap();
        assert_eq!(routes, ws, "workspace routes win wholesale");
    }

    #[tokio::test]
    async fn no_routes_anywhere_is_an_error() {
        use system_traits::impls::RealSys;

        let dir = tempfile::tempdir().unwrap();
        let result =
            resolve_effective_routes(&RealSys, "id", None, dir.path()).await;
        assert!(
            result.is_err(),
            "no workspace routes and no manifest is an error"
        );
    }

    #[tokio::test]
    async fn owned_routes_cannot_escalate_privileges() {
        use system_traits::impls::RealSys;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("projection.omni.yaml"),
            "routes:\n  - strategy: namespaced\n    allow_git: true\n",
        )
        .unwrap();

        let result =
            resolve_effective_routes(&RealSys, "id", None, dir.path()).await;
        assert!(
            result.is_err(),
            "an owned route setting allow_git must be rejected"
        );
    }

    #[tokio::test]
    async fn workspace_routes_may_set_allow_flags() {
        use system_traits::impls::RealSys;

        let dir = tempfile::tempdir().unwrap();
        let ws = vec![route(r#"{"strategy":"namespaced","allow_git":true}"#)];
        let routes =
            resolve_effective_routes(&RealSys, "id", Some(&ws), dir.path())
                .await
                .unwrap();
        assert_eq!(routes, ws, "workspace config may relax the safety floor");
    }
}
