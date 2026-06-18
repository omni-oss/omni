use std::collections::BTreeMap;

use omni_context::{Context, ContextSys, GetVarsArgs};
use serde::{Deserialize, Serialize};

// ── Request ───────────────────────────────────────────────────────────────────

/// Request to retrieve workspace environment variables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnvRequest {
    /// If set, return only this key. If `None`, return all variables.
    pub key: Option<String>,
}

// ── Response ──────────────────────────────────────────────────────────────────

/// Workspace environment variables.
#[derive(Debug, Serialize, Deserialize)]
pub struct EnvResponse {
    /// All resolved environment variables, sorted by key.
    pub vars: BTreeMap<String, String>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Retrieve workspace environment variables (synchronous, no task execution).
pub fn handle_env<TSys: ContextSys>(
    ctx: &Context<TSys>,
    req: EnvRequest,
) -> eyre::Result<EnvResponse> {
    let mut env_loader = ctx.create_env_loader();
    let env_vars = env_loader.get(&GetVarsArgs {
        inherit_env_vars: ctx.inherit_env_vars(),
        ..Default::default()
    })?;

    let vars: BTreeMap<String, String> = if let Some(key) = req.key {
        env_vars
            .get(&key)
            .map(|v| BTreeMap::from([(key, v.clone())]))
            .unwrap_or_default()
    } else {
        env_vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };

    Ok(EnvResponse { vars })
}
