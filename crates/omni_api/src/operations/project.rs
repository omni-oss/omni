use omni_configurations::ProjectConfiguration;
use omni_context::{Context, ContextSys};

// ── Handlers ─────────────────────────────────────────────────────────────────

/// List the names of all projects in the workspace.
pub async fn handle_project_list<TSys: ContextSys>(
    ctx: &Context<TSys>,
) -> eyre::Result<Vec<String>> {
    let loaded = ctx.load_project_configurations().await?;
    Ok(loaded.into_iter().map(|p| p.name).collect())
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
