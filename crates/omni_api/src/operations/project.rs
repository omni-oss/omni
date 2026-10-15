use std::path::{Path, PathBuf};

use omni_configurations::ProjectConfiguration;
use omni_context::{Context, ContextSys, ProjectConfigEntry};
use path_clean::clean;
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

/// Selects one loaded project configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSelector {
    /// The config with this project name.
    Name(String),
    /// The config stored at this absolute path; callers resolve relative
    /// input before building the selector.
    Path(PathBuf),
}

// ── Response ──────────────────────────────────────────────────────────────────

/// A selected project configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectConfigResponse {
    pub config: ProjectConfiguration,
    /// Non-fatal notes about the selection, such as other configs sharing
    /// the requested name.
    pub warnings: Vec<String>,
}

impl ProjectConfigResponse {
    fn new(config: ProjectConfiguration) -> Self {
        Self {
            config,
            warnings: Vec::new(),
        }
    }
}

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

/// Return the full configuration selected by `selector`.
///
/// Every loaded config is searched, including base templates and configs
/// reached only through `extends`. When a name matches several configs, a
/// single runnable project wins; two or more runnable projects are an error;
/// otherwise the first match by path is returned with a warning.
pub async fn handle_project_config<TSys: ContextSys>(
    ctx: &Context<TSys>,
    selector: ProjectSelector,
) -> eyre::Result<ProjectConfigResponse> {
    let entries = ctx.load_project_config_entries().await?;

    match selector {
        ProjectSelector::Name(name) => {
            select_by_name(ctx.root_dir(), entries, &name)
        }
        ProjectSelector::Path(path) => select_by_path(entries, &path),
    }
}

fn select_by_name(
    root_dir: &Path,
    entries: Vec<ProjectConfigEntry>,
    name: &str,
) -> eyre::Result<ProjectConfigResponse> {
    let mut matches = entries
        .into_iter()
        .filter(|entry| entry.config.name == name)
        .map(|entry| {
            let path = workspace_relative_path(root_dir, &entry.config)?;
            Ok((path, entry))
        })
        .collect::<eyre::Result<Vec<_>>>()?;
    matches.sort_by(|(a, _), (b, _)| a.cmp(b));

    let (mut runnable, templates): (Vec<_>, Vec<_>) = matches
        .into_iter()
        .partition(|(_, entry)| entry.is_runnable());

    if runnable.len() > 1 {
        let paths = runnable
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>();
        eyre::bail!(
            "duplicate project name '{name}' defined by: {}. Use --path to select one",
            paths.join(", ")
        );
    }

    if let Some((_, entry)) = runnable.pop() {
        return Ok(ProjectConfigResponse::new(entry.config));
    }

    let mut templates = templates.into_iter();
    let (selected_path, selected) = templates
        .next()
        .ok_or_else(|| eyre::eyre!("No project named '{}' found", name))?;
    let others = templates.map(|(path, _)| path).collect::<Vec<_>>();

    let mut response = ProjectConfigResponse::new(selected.config);
    if !others.is_empty() {
        response.warnings.push(format!(
            "multiple configurations named '{name}' found; showing {selected_path}. Other matches: {}. Use --path to select one",
            others.join(", ")
        ));
    }

    Ok(response)
}

fn select_by_path(
    entries: Vec<ProjectConfigEntry>,
    path: &Path,
) -> eyre::Result<ProjectConfigResponse> {
    let path = clean(path);

    entries
        .into_iter()
        .find(|entry| {
            entry
                .config
                .file
                .path()
                .is_ok_and(|file| file == path.as_path())
        })
        .map(|entry| ProjectConfigResponse::new(entry.config))
        .ok_or_else(|| {
            eyre::eyre!("no loaded project configuration at {}", path.display())
        })
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
