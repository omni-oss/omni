//! The benchmark scenario matrix.
//!
//! The default matrix is deliberately **varied** so the suite exercises every
//! area where a task-execution regression could hide: graph scale, graph shape,
//! and task density. It is kept curated (moderate sizes, no full cross-product)
//! so `cargo bench -p omni_bench` runs in a few minutes and is CI-friendly; the
//! `full-matrix` feature expands scale for deeper investigations.

use omni_test_utils::{
    DependencyStrategy, HarnessConfig, presets::exec_preset,
};

/// One point in the matrix: a named workspace configuration.
pub struct MatrixEntry {
    pub name: String,
    pub config: HarnessConfig,
}

impl MatrixEntry {
    fn new(config: HarnessConfig) -> Self {
        Self {
            name: config.workspace_name.clone(),
            config,
        }
    }

    /// The leaf task to invoke — the last task in each project, so its whole
    /// intra-project chain (and, transitively, upstream projects) executes.
    pub fn leaf_task(&self) -> String {
        format!("t{}", self.config.tasks_per_project.saturating_sub(1))
    }
}

/// The default, varied matrix: scale sweep + shape sweep + density sweep.
/// De-duplicated by workspace name.
pub fn default_matrix() -> Vec<MatrixEntry> {
    use DependencyStrategy::*;

    let mut entries: Vec<HarnessConfig> = Vec::new();

    // Scale sweep (layered, 3 tasks): how overhead grows with graph size.
    let scales: &[usize] = if cfg!(feature = "full-matrix") {
        &[30, 120, 300, 600]
    } else {
        &[30, 120]
    };
    for &projects in scales {
        entries.push(exec_preset(projects, 3, Layered));
    }

    // Shape sweep (fixed 120 projects, 3 tasks): how graph shape affects
    // batching / available parallelism.
    for strategy in [Isolated, Chain, FanOut, Layered, Random] {
        entries.push(exec_preset(120, 3, strategy));
    }

    // Density sweep (120 projects, layered): tasks per project.
    let densities: &[usize] = if cfg!(feature = "full-matrix") {
        &[1, 5, 10]
    } else {
        &[1, 5]
    };
    for &tasks in densities {
        entries.push(exec_preset(120, tasks, Layered));
    }

    dedup_by_name(entries)
}

/// A small representative subset for the expensive scenarios (cold/no-cache),
/// which spawn real processes each iteration. Deep (chain) + wide (layered).
pub fn heavy_subset() -> Vec<MatrixEntry> {
    use DependencyStrategy::*;
    dedup_by_name(vec![
        exec_preset(120, 3, Layered),
        exec_preset(120, 3, Chain),
    ])
}

/// Concurrency values to sweep for the scheduler-overhead group.
pub fn concurrency_values() -> Vec<usize> {
    let cpus = num_cpus::get();
    let mut v = vec![1, 4];
    if !v.contains(&cpus) {
        v.push(cpus);
    }
    v
}

fn dedup_by_name(configs: Vec<HarnessConfig>) -> Vec<MatrixEntry> {
    let mut seen = std::collections::BTreeSet::new();
    configs
        .into_iter()
        .filter(|c| seen.insert(c.workspace_name.clone()))
        .map(MatrixEntry::new)
        .collect()
}
