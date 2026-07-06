//! High-level, task-bench-style workspace generation.
//!
//! [`generate_workspace`] takes a single [`HarnessConfig`] and writes a
//! complete omni workspace to disk: N projects wired into the configured
//! dependency-graph shape, each with `t0..t{M-1}` tasks whose intra-/inter-
//! project dependencies mirror `scripts/task-bench`, cache enabled with real
//! `src/**` inputs and `dist/**` outputs.
//!
//! # Determinism
//!
//! For a given [`HarnessConfig`] the output is byte-identical across runs: the
//! graph is seeded ([`crate::graph`]) and every generated collection is
//! ordered. This is required so benchmarks measure code changes, not workload
//! drift.

use std::{collections::BTreeMap, path::Path};

use crate::{
    CacheConfigurationGenerator, CacheKeyConfigurationGenerator, HarnessConfig,
    ProjectGenerator, ProjectNode, TaskGenerator,
    TaskOutputConfigurationGenerator, WorkspaceGenerator, build_graph,
    project_launcher, task_command, task_names,
};

/// Generate a complete benchmark workspace at `dir` from `config`.
///
/// Returns the generated project graph (useful for asserting the expected
/// task-graph size in a benchmark harness).
pub fn generate_workspace(
    dir: impl AsRef<Path>,
    config: &HarnessConfig,
) -> eyre::Result<Vec<ProjectNode>> {
    let nodes = build_graph(config);
    let tasks = task_names(config.tasks_per_project);

    let mut projects = Vec::with_capacity(nodes.len());

    for node in &nodes {
        let dependencies = node
            .dependencies
            .iter()
            .map(|&i| nodes[i].name.clone())
            .collect::<Vec<_>>();

        let launcher = project_launcher(&node.name, config.task.output_files);

        let mut task_map = BTreeMap::new();
        for (task_index, task_name) in tasks.iter().enumerate() {
            let mut task_deps = Vec::new();

            // Intra-project chain: tN depends on t(N-1).
            if config.task.chain_within_project && task_index > 0 {
                task_deps.push(tasks[task_index - 1].clone());
            }

            // Inter-project fan-up: tN depends on ^tN of upstream projects.
            if config.task.fan_upstream {
                task_deps.push(format!("^{task_name}"));
            }

            let command = task_command(launcher.script_name, task_name);

            let output_files = (0..config.task.output_files)
                .map(|f| format!("./dist/{task_name}.{f}.txt"))
                .collect::<Vec<_>>();

            let task = TaskGenerator::builder()
                .command(command)
                .dependencies(task_deps)
                .cache(
                    CacheConfigurationGenerator::builder()
                        .enabled(config.cache_enabled)
                        .build(),
                )
                .output(
                    TaskOutputConfigurationGenerator::builder()
                        .files(output_files)
                        .build(),
                )
                .build();

            task_map.insert(task_name.clone(), task);
        }

        let extra_files = BTreeMap::from([(
            launcher.script_name.to_string(),
            launcher.script_body,
        )]);

        let project = ProjectGenerator::builder()
            .name(node.name.clone())
            .dependencies(dependencies)
            .tasks(task_map)
            .extra_files(extra_files)
            .cache(
                CacheConfigurationGenerator::builder()
                    .enabled(config.cache_enabled)
                    .key(
                        CacheKeyConfigurationGenerator::builder()
                            .defaults(true)
                            .files(vec!["./src/**/*.*".to_string()])
                            .build(),
                    )
                    .build(),
            )
            .folder_nesting(config.content.folder_nesting)
            .leaf_folder_count(config.content.leaf_folder_count)
            .file_count_per_leaf_folder(config.content.files_per_leaf_folder)
            .file_extension(config.content.file_extension.clone())
            .file_content(config.content.file_content.clone())
            .build();

        projects.push(project);
    }

    WorkspaceGenerator::builder()
        .name(config.workspace_name.clone())
        .projects(projects)
        .build()
        .generate(dir)?;

    Ok(nodes)
}
