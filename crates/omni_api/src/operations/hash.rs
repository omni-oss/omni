use omni_context::{ContextSys, LoadedContext};
use omni_execution_plan::TaskSelector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Response ──────────────────────────────────────────────────────────────────

/// The computed hash string.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HashResponse {
    pub hash: String,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// Compute the hash for the entire workspace.
pub async fn handle_hash_workspace<TSys: ContextSys>(
    ctx: &LoadedContext<TSys>,
) -> eyre::Result<HashResponse> {
    Ok(HashResponse {
        hash: ctx.get_workspace_hash_string().await?,
    })
}

/// Compute the hash for a single project.
///
/// With no selectors all tasks are hashed together. Otherwise only the tasks
/// named in `tasks` (exact; glob characters are literal) or matching a glob in
/// `task_patterns` are included. An exact name that is not a task of the
/// project is an error.
pub async fn handle_hash_project<TSys: ContextSys>(
    ctx: &LoadedContext<TSys>,
    name: &str,
    tasks: &[String],
    task_patterns: &[String],
) -> eyre::Result<HashResponse> {
    let selectors: Vec<TaskSelector> = tasks
        .iter()
        .map(TaskSelector::exact)
        .chain(task_patterns.iter().map(TaskSelector::pattern))
        .collect();
    Ok(HashResponse {
        hash: ctx.get_project_hash_string(name, &selectors).await?,
    })
}
