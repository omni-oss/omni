use std::path::PathBuf;

use crate::constants;

/// Workspace initialization parameters, decoupled from clap's CliArgs.
/// Build this from whatever input source (CLI args, HTTP request body, etc.)
/// and pass it to context-creation functions.
#[derive(Debug, Clone)]
pub struct WorkspaceInitConfig {
    pub env: String,
    pub env_files: Option<Vec<PathBuf>>,
    /// File name that marks the workspace root dir (walked upward).
    pub env_root_dir_marker: String,
    pub inherit_env_vars: bool,
}

impl Default for WorkspaceInitConfig {
    fn default() -> Self {
        Self {
            env: "development".to_string(),
            env_files: None,
            env_root_dir_marker: constants::WORKSPACE_OMNI
                .replace("{ext}", "yaml"),
            inherit_env_vars: false,
        }
    }
}
