//! High-level, task-bench-style workspace generation.
//!
//! [`generate_workspace`] takes a single [`HarnessConfig`] and writes a
//! complete omni workspace to disk. The graph, task edges, and omni config are
//! computed by the pure [`omni_workspace_gen`] core ([`build_model`] +
//! [`render_omni`]); this host writes the rendered omni files and adds the
//! host-only neutral base (a launcher script per project + the `src/**` input
//! tree).
//!
//! # Determinism
//!
//! For a given [`HarnessConfig`] the output is byte-identical across runs: the
//! model is derived from a seeded graph, every collection is ordered, and the
//! content tree is deterministic. This is required so benchmarks measure code
//! changes, not workload drift.

use std::{fs, path::Path};

use omni_workspace_gen::{
    HarnessConfig, OmniRenderOptions, ProjectNode, build_graph, build_model,
    render_omni,
};

use crate::{project_launcher, task_command_template, write_content_tree};

/// Generate a complete benchmark workspace at `dir` from `config`.
///
/// Returns the generated project graph (useful for asserting the expected
/// task-graph size in a benchmark harness).
pub fn generate_workspace(
    dir: impl AsRef<Path>,
    config: &HarnessConfig,
) -> eyre::Result<Vec<ProjectNode>> {
    let dir = dir.as_ref();
    let model = build_model(config);

    // The omni layer is shared with task-bench; only the task command (which
    // invokes this host's launcher) and the cache-key inputs (this host's
    // `src/**` tree) are host-specific.
    let options = OmniRenderOptions {
        task_command_template: task_command_template(),
        project_cache_key_files: vec!["./src/**/*.*".to_string()],
    };

    for (rel_path, contents) in render_omni(&model, &options) {
        let path = dir.join(&rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }

    for project in &model.projects {
        let project_dir = dir.join(&project.dir);
        fs::create_dir_all(&project_dir)?;

        let launcher =
            project_launcher(&project.name, config.task.output_files);
        fs::write(
            project_dir.join(launcher.script_name),
            launcher.script_body,
        )?;

        write_content_tree(
            project_dir.join("src"),
            config.content.folder_nesting,
            config.content.leaf_folder_count,
            config.content.files_per_leaf_folder,
            &config.content.file_extension,
            &config.content.file_content,
        )?;
    }

    Ok(build_graph(config))
}
