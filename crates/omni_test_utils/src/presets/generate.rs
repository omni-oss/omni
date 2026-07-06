use std::path::Path;

use crate::{HarnessConfig, ProjectNode, generate_workspace};

/// Generate a workspace on disk from a preset (a [`HarnessConfig`]).
///
/// Returns the generated project graph. Thin wrapper over
/// [`generate_workspace`].
pub fn generate(
    dir: impl AsRef<Path>,
    config: &HarnessConfig,
) -> eyre::Result<Vec<ProjectNode>> {
    generate_workspace(dir, config)
}
