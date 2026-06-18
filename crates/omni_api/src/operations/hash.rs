use omni_context::{ContextSys, LoadedContext};
use serde::{Deserialize, Serialize};

// ── Response ──────────────────────────────────────────────────────────────────

/// The computed hash string.
#[derive(Debug, Serialize, Deserialize)]
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
/// If `tasks` is empty all tasks are hashed together; otherwise only the
/// listed task names are included.
pub async fn handle_hash_project<TSys: ContextSys>(
    ctx: &LoadedContext<TSys>,
    name: &str,
    tasks: &[String],
) -> eyre::Result<HashResponse> {
    let task_refs: Vec<&str> = tasks.iter().map(String::as_str).collect();
    Ok(HashResponse {
        hash: ctx.get_project_hash_string(name, &task_refs).await?,
    })
}
