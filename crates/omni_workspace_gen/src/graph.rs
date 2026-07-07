//! Deterministic project-dependency graph generation.
//!
//! Produces the supported graph shapes (isolated / chain / fan-out / layered /
//! random) so generated omni workspaces mirror the ones the cross-tool
//! `task-bench` harness produces. `Random` graphs are reproducible for a given
//! seed via a `mulberry32` PRNG.

use serde::{Deserialize, Serialize};

use crate::HarnessConfig;

/// The supported inter-project dependency graph shapes.
///
/// These control how much of a task graph the executor has to walk during
/// scheduling, which is what the benchmarks exercise.
///
/// The wire representation is kebab-case (`fan-out`) so it matches the JS
/// ecosystem across the wasm boundary; `strum` shares that spelling.
#[derive(
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    strum::IntoStaticStr,
    strum::Display,
    strum::EnumIs,
    strum::EnumString,
)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyStrategy {
    /// No dependencies between projects.
    #[strum(serialize = "isolated")]
    Isolated,
    /// Each project depends on the immediately preceding project.
    #[strum(serialize = "chain")]
    Chain,
    /// Every project (except the root) depends on the single root project.
    #[strum(serialize = "fan-out")]
    FanOut,
    /// Projects are grouped into layers; each depends on an evenly-sampled
    /// subset of the previous layer.
    #[default]
    #[strum(serialize = "layered")]
    Layered,
    /// Each project depends on lower-indexed projects with a fixed probability.
    #[strum(serialize = "random")]
    Random,
}

/// A single generated project and the upstream projects it depends on.
#[derive(Debug, Clone)]
pub struct ProjectNode {
    /// Zero-based index in the generated set.
    pub index: usize,
    /// Project name, e.g. `p-0007`.
    pub name: String,
    /// Indices of upstream projects this project depends on.
    pub dependencies: Vec<usize>,
}

/// Deterministic PRNG (`mulberry32`) so a given seed always yields the same
/// graph. Its arithmetic matches the TypeScript `makeRng`.
struct Mulberry32 {
    a: u32,
}

impl Mulberry32 {
    fn new(seed: u32) -> Self {
        Self { a: seed }
    }

    /// Returns the next pseudo-random `f64` in `[0, 1)`.
    fn next_unit(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6d2b79f5);
        let mut t = (self.a ^ (self.a >> 15)).wrapping_mul(1 | self.a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        ((t ^ (t >> 14)) as f64) / 4_294_967_296.0
    }
}

/// Zero-padding width for project indices: the number of decimal digits in the
/// project count, so names sort lexicographically.
fn pad_width(projects: usize) -> usize {
    projects.to_string().len()
}

/// The name of the project at `index`, expanding the configured
/// `project_name_template` (`{project_id}` -> zero-padded index).
pub fn project_name(config: &HarnessConfig, index: usize) -> String {
    let width = pad_width(config.projects);
    let id = format!("{index:0width$}");
    config.project_name_template.replace("{project_id}", &id)
}

/// The task names generated for every project: `t0`, `t1`, ...
pub fn task_names(tasks_per_project: usize) -> Vec<String> {
    (0..tasks_per_project).map(|i| format!("t{i}")).collect()
}

/// Evenly sample up to `count` indices from the inclusive range
/// `[start, end]`.
fn evenly_sample(start: usize, end: usize, count: usize) -> Vec<usize> {
    if end < start || count == 0 {
        return vec![];
    }
    let size = end - start + 1;
    if count >= size {
        return (start..=end).collect();
    }

    let mut picked = Vec::with_capacity(count);
    let denom = (count - 1).max(1) as f64;
    for i in 0..count {
        // Spread picks across the range deterministically (round-half-away).
        let offset = ((i * (size - 1)) as f64 / denom).round() as usize;
        let value = start + offset;
        if !picked.contains(&value) {
            picked.push(value);
        }
    }
    picked
}

fn compute_dependencies(
    config: &HarnessConfig,
    index: usize,
    rng: &mut Mulberry32,
) -> Vec<usize> {
    let dep = &config.dependency;
    match dep.strategy {
        DependencyStrategy::Isolated => vec![],
        DependencyStrategy::Chain => {
            if index > 0 {
                vec![index - 1]
            } else {
                vec![]
            }
        }
        DependencyStrategy::FanOut => {
            if index > 0 {
                vec![0]
            } else {
                vec![]
            }
        }
        DependencyStrategy::Layered => {
            let per_layer = config.projects.div_ceil(dep.layers.max(1));
            if per_layer == 0 {
                return vec![];
            }
            let layer = index / per_layer;
            if layer == 0 {
                return vec![];
            }
            let prev_start = (layer - 1) * per_layer;
            let prev_end = (layer * per_layer).min(config.projects) - 1;
            evenly_sample(prev_start, prev_end, dep.fanout)
        }
        DependencyStrategy::Random => {
            if index == 0 {
                return vec![];
            }
            let mut deps = Vec::new();
            for j in 0..index {
                if rng.next_unit() < dep.edge_probability {
                    deps.push(j);
                }
            }
            if dep.fanout > 0 && deps.len() > dep.fanout {
                // Keep the `fanout` closest ancestors for a shallower graph.
                deps.split_off(deps.len() - dep.fanout)
            } else {
                deps
            }
        }
    }
}

/// Build the full project graph for a config. Dependencies always point to
/// lower indices, guaranteeing an acyclic graph.
pub fn build_graph(config: &HarnessConfig) -> Vec<ProjectNode> {
    let mut rng = Mulberry32::new(config.seed);
    (0..config.projects)
        .map(|index| ProjectNode {
            index,
            name: project_name(config, index),
            dependencies: compute_dependencies(config, index, &mut rng),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DependencyConfig, HarnessConfig};

    fn config(strategy: DependencyStrategy, projects: usize) -> HarnessConfig {
        HarnessConfig::builder()
            .projects(projects)
            .dependency(DependencyConfig::builder().strategy(strategy).build())
            .build()
    }

    #[test]
    fn isolated_has_no_edges() {
        let g = build_graph(&config(DependencyStrategy::Isolated, 10));
        assert!(g.iter().all(|n| n.dependencies.is_empty()));
    }

    #[test]
    fn chain_links_previous() {
        let g = build_graph(&config(DependencyStrategy::Chain, 5));
        assert_eq!(g[0].dependencies, Vec::<usize>::new());
        assert_eq!(g[1].dependencies, vec![0]);
        assert_eq!(g[4].dependencies, vec![3]);
    }

    #[test]
    fn fan_out_targets_root() {
        let g = build_graph(&config(DependencyStrategy::FanOut, 4));
        assert_eq!(g[0].dependencies, Vec::<usize>::new());
        assert!(g[1..].iter().all(|n| n.dependencies == vec![0]));
    }

    #[test]
    fn all_edges_point_to_lower_indices() {
        let g = build_graph(&config(DependencyStrategy::Random, 50));
        for node in &g {
            assert!(node.dependencies.iter().all(|&d| d < node.index));
        }
    }

    #[test]
    fn random_is_deterministic_for_seed() {
        let a = build_graph(&config(DependencyStrategy::Random, 30));
        let b = build_graph(&config(DependencyStrategy::Random, 30));
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.dependencies, y.dependencies);
        }
    }

    #[test]
    fn task_names_are_sequential() {
        assert_eq!(task_names(3), vec!["t0", "t1", "t2"]);
    }

    #[test]
    fn project_name_uses_template_and_digit_padding() {
        let c = HarnessConfig::builder().projects(12).build();
        assert_eq!(project_name(&c, 7), "p-07");

        let c = HarnessConfig::builder().projects(1000).build();
        assert_eq!(project_name(&c, 7), "p-0007");

        let c = HarnessConfig::builder().projects(5).build();
        assert_eq!(project_name(&c, 3), "p-3");
    }

    #[test]
    fn project_name_respects_custom_template() {
        let c = HarnessConfig::builder()
            .projects(100)
            .project_name_template("bench-p{project_id}")
            .build();
        assert_eq!(project_name(&c, 4), "bench-p004");
    }

    #[test]
    fn strategy_serializes_as_kebab_case() {
        assert_eq!(
            serde_json::to_string(&DependencyStrategy::FanOut).unwrap(),
            "\"fan-out\""
        );
        assert_eq!(
            serde_json::from_str::<DependencyStrategy>("\"fan-out\"").unwrap(),
            DependencyStrategy::FanOut
        );
    }
}
