//! The serializable workspace model — the boundary contract.
//!
//! [`build_model`] is the single source of truth for the graph, deterministic
//! naming, task-graph edges, output declarations, and the cold-run task counts.
//! It performs **no** filesystem access: it returns a value the host serializes
//! or lays down as files. All collections are ordered ([`Vec`] in index order,
//! [`BTreeMap`] by key) so the serialized payload is byte-stable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{HarnessConfig, build_graph, task_names};

/// Schema version of the [`WorkspaceModel`] payload; bumped on breaking
/// changes so a consumer built against an older schema can fail loudly.
pub const MODEL_VERSION: u32 = 1;

/// The complete, filesystem-free description of a benchmark workspace.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorkspaceModel {
    /// Schema version of this payload; see [`MODEL_VERSION`].
    pub model_version: u32,
    /// The fully-resolved config (defaults applied) that produced this model.
    pub config: HarnessConfig,
    /// Generated projects, in index order.
    pub projects: Vec<ProjectModel>,
    /// Deterministic cold-run task counts per task name (`t0`, `t1`, ...), so
    /// external harnesses can verify cache-hit behavior without re-deriving
    /// the rule.
    pub expected_cold_executed: BTreeMap<String, usize>,
}

/// A single generated project.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProjectModel {
    /// Zero-based index in the generated set.
    pub index: usize,
    /// Project name, e.g. `p-0007`.
    pub name: String,
    /// Workspace-relative POSIX dir, e.g. `packages/p-0007`.
    pub dir: String,
    /// Upstream project *names* (resolved from indices).
    pub dependencies: Vec<String>,
    /// Ordered task graph: `t0..t{K-1}` with resolved edges + outputs.
    pub tasks: Vec<TaskModel>,
}

/// A single task within a project.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskModel {
    /// Task name, e.g. `t0`.
    pub name: String,
    /// Resolved dependency edges: intra-project (`t{k-1}`) then upstream
    /// (`^t{k}`).
    pub dependencies: Vec<String>,
    /// Declared cache output globs, e.g. `["dist/t1.*"]`.
    pub output_globs: Vec<String>,
}

/// Build the workspace model for `config`. Pure: the single source of truth for
/// graph, naming, task edges, outputs, and cold-run math.
pub fn build_model(config: &HarnessConfig) -> WorkspaceModel {
    let nodes = build_graph(config);
    let tasks = task_names(config.tasks_per_project);

    let projects = nodes
        .iter()
        .map(|node| {
            let dependencies = node
                .dependencies
                .iter()
                .map(|&i| nodes[i].name.clone())
                .collect::<Vec<_>>();

            let task_models = tasks
                .iter()
                .enumerate()
                .map(|(task_index, task_name)| {
                    let mut dependencies = Vec::new();

                    // Intra-project chain: tN depends on t(N-1).
                    if config.task.chain_within_project && task_index > 0 {
                        dependencies.push(tasks[task_index - 1].clone());
                    }

                    // Inter-project fan-up: tN depends on ^tN of upstream
                    // projects.
                    if config.task.fan_upstream {
                        dependencies.push(format!("^{task_name}"));
                    }

                    TaskModel {
                        name: task_name.clone(),
                        dependencies,
                        output_globs: vec![format!("dist/{task_name}.*")],
                    }
                })
                .collect::<Vec<_>>();

            ProjectModel {
                index: node.index,
                name: node.name.clone(),
                dir: format!("packages/{}", node.name),
                dependencies,
                tasks: task_models,
            }
        })
        .collect::<Vec<_>>();

    let expected_cold_executed = tasks
        .iter()
        .enumerate()
        .map(|(k, name)| {
            let per_project = if config.task.chain_within_project {
                k + 1
            } else {
                1
            };
            (name.clone(), config.projects * per_project)
        })
        .collect::<BTreeMap<_, _>>();

    WorkspaceModel {
        model_version: MODEL_VERSION,
        config: config.clone(),
        projects,
        expected_cold_executed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DependencyConfig, DependencyStrategy, TaskConfig};

    fn config(
        strategy: DependencyStrategy,
        projects: usize,
        tasks: usize,
    ) -> HarnessConfig {
        HarnessConfig::builder()
            .projects(projects)
            .tasks_per_project(tasks)
            .dependency(DependencyConfig::builder().strategy(strategy).build())
            .build()
    }

    #[test]
    fn task_edges_chain_and_fan_upstream() {
        let model = build_model(&config(DependencyStrategy::Isolated, 2, 3));
        let tasks = &model.projects[0].tasks;

        assert_eq!(tasks[0].dependencies, vec!["^t0"]);
        assert_eq!(tasks[1].dependencies, vec!["t0", "^t1"]);
        assert_eq!(tasks[2].dependencies, vec!["t1", "^t2"]);
    }

    #[test]
    fn task_edges_without_chain_or_fan() {
        let cfg = HarnessConfig::builder()
            .projects(2)
            .tasks_per_project(3)
            .task(
                TaskConfig::builder()
                    .chain_within_project(false)
                    .fan_upstream(false)
                    .build(),
            )
            .build();
        let model = build_model(&cfg);

        for task in &model.projects[0].tasks {
            assert!(task.dependencies.is_empty());
        }
    }

    #[test]
    fn dir_is_packages_slash_name() {
        let model = build_model(&config(DependencyStrategy::Isolated, 1000, 1));
        assert_eq!(model.projects[7].name, "p-0007");
        assert_eq!(model.projects[7].dir, "packages/p-0007");
    }

    #[test]
    fn output_globs_cover_each_task() {
        let model = build_model(&config(DependencyStrategy::Isolated, 1, 2));
        assert_eq!(model.projects[0].tasks[0].output_globs, vec!["dist/t0.*"]);
        assert_eq!(model.projects[0].tasks[1].output_globs, vec!["dist/t1.*"]);
    }

    #[test]
    fn project_dependencies_resolve_to_names() {
        let model = build_model(&config(DependencyStrategy::Chain, 3, 1));
        assert_eq!(model.projects[0].dependencies, Vec::<String>::new());
        assert_eq!(model.projects[1].dependencies, vec!["p-0"]);
        assert_eq!(model.projects[2].dependencies, vec!["p-1"]);
    }

    #[test]
    fn expected_cold_executed_with_chain() {
        let model = build_model(&config(DependencyStrategy::Isolated, 10, 3));
        assert_eq!(model.expected_cold_executed["t0"], 10);
        assert_eq!(model.expected_cold_executed["t1"], 20);
        assert_eq!(model.expected_cold_executed["t2"], 30);
    }

    #[test]
    fn expected_cold_executed_without_chain() {
        let cfg = HarnessConfig::builder()
            .projects(10)
            .tasks_per_project(3)
            .task(TaskConfig::builder().chain_within_project(false).build())
            .build();
        let model = build_model(&cfg);

        for k in 0..3 {
            assert_eq!(model.expected_cold_executed[&format!("t{k}")], 10);
        }
    }

    #[test]
    fn serialized_model_is_byte_stable() {
        let cfg = config(DependencyStrategy::Random, 40, 3);
        let a = serde_json::to_string(&build_model(&cfg)).unwrap();
        let b = serde_json::to_string(&build_model(&cfg)).unwrap();
        assert_eq!(a, b);

        // Round-trips without loss.
        let back: WorkspaceModel = serde_json::from_str(&a).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), a);
    }

    #[test]
    fn model_carries_version_and_config() {
        let cfg = config(DependencyStrategy::Layered, 5, 2);
        let model = build_model(&cfg);
        assert_eq!(model.model_version, MODEL_VERSION);
        assert_eq!(model.config, cfg);
    }
}
