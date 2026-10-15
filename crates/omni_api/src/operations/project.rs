use std::path::Path;

use omni_configurations::ProjectConfiguration;
use omni_context::{Context, ContextSys, ProjectConfigEntry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Request ───────────────────────────────────────────────────────────────────

/// Which project configurations to list besides the runnable projects.
///
/// Every flag only adds entries, so they combine with OR.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProjectListRequest {
    /// Also list configs marked `base: true`.
    #[serde(default)]
    pub include_base: bool,
    /// Also list configs reached only through `extends`.
    #[serde(default)]
    pub include_extended: bool,
    /// List every loaded config.
    #[serde(default)]
    pub all: bool,
}

impl ProjectListRequest {
    fn includes(&self, entry: &ProjectConfigEntry) -> bool {
        self.all
            || entry.is_runnable()
            || (self.include_base && entry.config.base)
            || (self.include_extended && !entry.discovered)
    }
}

// ── Response ──────────────────────────────────────────────────────────────────

/// A listed project configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProjectListEntry {
    pub name: String,
    /// Workspace-relative, `/`-separated path to the config file.
    pub path: String,
    /// Whether the config is marked `base: true`.
    pub base: bool,
    /// Whether project discovery found the config, as opposed to it being
    /// reached only through `extends`.
    pub discovered: bool,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// List the project configurations selected by `req`, sorted by name and
/// path. With a default request only runnable projects are listed.
pub async fn handle_project_list<TSys: ContextSys>(
    ctx: &Context<TSys>,
    req: ProjectListRequest,
) -> eyre::Result<Vec<ProjectListEntry>> {
    let entries = ctx.load_project_config_entries().await?;

    let mut listed = entries
        .iter()
        .filter(|entry| req.includes(entry))
        .map(|entry| {
            Ok(ProjectListEntry {
                name: entry.config.name.clone(),
                path: workspace_relative_path(ctx.root_dir(), &entry.config)?,
                base: entry.config.base,
                discovered: entry.discovered,
            })
        })
        .collect::<eyre::Result<Vec<_>>>()?;

    listed.sort_by(|a, b| (&a.name, &a.path).cmp(&(&b.name, &b.path)));

    Ok(listed)
}

/// Return the full configuration for the named project.
///
/// Returns an error if no project with that name exists.
pub async fn handle_project_config<TSys: ContextSys>(
    ctx: &Context<TSys>,
    name: &str,
) -> eyre::Result<ProjectConfiguration> {
    let loaded = ctx.load_project_configurations().await?;
    loaded
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| eyre::eyre!("No project named '{}' found", name))
}

fn workspace_relative_path(
    root_dir: &Path,
    config: &ProjectConfiguration,
) -> eyre::Result<String> {
    let file = config.file.path()?;
    let relative = file.strip_prefix(root_dir).map_err(|_| {
        eyre::eyre!(
            "project configuration {} is outside the workspace root {}",
            file.display(),
            root_dir.display()
        )
    })?;

    Ok(relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}
