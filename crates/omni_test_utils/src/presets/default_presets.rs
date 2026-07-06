use std::sync::LazyLock;

use crate::{
    ContentConfig, DependencyConfig, DependencyStrategy, HarnessConfig,
    TaskConfig,
};

/// A large JS-flavored workspace (1000 isolated, single-task projects).
///
/// Kept isolated with one task to preserve the context-loading benchmark's
/// original semantics.
pub static JS_LARGE: LazyLock<HarnessConfig> =
    LazyLock::new(|| js_preset("js_large", 1000, 5, 10));

/// A medium JS-flavored workspace (500 isolated, single-task projects).
pub static JS_MEDIUM: LazyLock<HarnessConfig> =
    LazyLock::new(|| js_preset("js_medium", 500, 2, 5));

/// A small JS-flavored workspace (100 isolated, single-task projects).
pub static JS_SMALL: LazyLock<HarnessConfig> =
    LazyLock::new(|| js_preset("js_small", 100, 0, 1));

fn js_preset(
    name: &str,
    projects: usize,
    folder_nesting: usize,
    leaf_folder_count: usize,
) -> HarnessConfig {
    HarnessConfig::builder()
        .workspace_name(name)
        .projects(projects)
        .tasks_per_project(1)
        .dependency(
            DependencyConfig::builder()
                .strategy(DependencyStrategy::Isolated)
                .build(),
        )
        .content(
            ContentConfig::builder()
                .folder_nesting(folder_nesting)
                .leaf_folder_count(leaf_folder_count)
                .files_per_leaf_folder(10)
                .file_extension("js")
                .file_content("console.log('Hello World!');")
                .build(),
        )
        .build()
}

/// Build an execution-oriented preset: `projects` projects, `tasks` tasks
/// each, wired into the given dependency-graph `strategy`. Suitable for the
/// task-execution benchmark matrix (real graphs, multiple tasks, cache on).
pub fn exec_preset(
    projects: usize,
    tasks: usize,
    strategy: DependencyStrategy,
) -> HarnessConfig {
    let shape: &'static str = strategy.into();

    HarnessConfig::builder()
        .workspace_name(format!("exec_{shape}_{projects}p_{tasks}t"))
        .projects(projects)
        .tasks_per_project(tasks)
        .dependency(DependencyConfig::builder().strategy(strategy).build())
        .task(
            TaskConfig::builder()
                .chain_within_project(true)
                .fan_upstream(true)
                .build(),
        )
        .content(
            ContentConfig::builder()
                .folder_nesting(1)
                .leaf_folder_count(2)
                .files_per_leaf_folder(5)
                .build(),
        )
        .build()
}
