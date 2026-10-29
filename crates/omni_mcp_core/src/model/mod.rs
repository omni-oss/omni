pub mod cache;
pub mod generator;
pub mod hash;
pub mod ignore;
pub mod project;
pub mod projection;
pub mod task;
pub mod tool;
pub mod workspace;

pub use cache::*;
pub use generator::*;
pub use hash::*;
pub use ignore::*;
pub use project::*;
pub use projection::*;
pub use task::*;
pub use tool::*;
pub use workspace::*;

/// MCP-only workspace selection; API operation requests stay workspace-agnostic.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WorkspaceParams<P> {
    /// Absolute existing invocation directory. Omni discovers the nearest
    /// workspace from its canonical (physical) ancestor chain.
    pub working_dir: std::path::PathBuf,
    #[serde(flatten)]
    pub params: P,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EmptyParams {}
