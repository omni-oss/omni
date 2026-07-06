//! Configuration for the workspace-generation harness.
//!
//! Mirrors the config surface of `scripts/task-bench` (see `config.ts`) so that
//! generated omni workspaces have the same knobs: project count, tasks per
//! project, dependency-graph shape, and the `src/**` input tree.
//!
//! # Determinism
//!
//! Every field here is a plain value and the graph is seeded (see
//! [`crate::graph`]), so a given [`HarnessConfig`] always produces a
//! **byte-identical** workspace. This is a hard requirement: the benchmarks
//! compare timings across runs, so the generated workload must not vary between
//! runs of the same preset. Randomness (the [`DependencyStrategy::Random`]
//! shape) is derived solely from [`HarnessConfig::seed`], and the generators
//! use ordered maps so serialization order is stable.
//!
//! [`DependencyStrategy::Random`]: crate::DependencyStrategy::Random

use crate::DependencyStrategy;

/// Top-level harness configuration. Build with [`HarnessConfig::builder`].
#[derive(Debug, Clone, bon::Builder)]
pub struct HarnessConfig {
    /// Name written into the workspace configuration.
    #[builder(default = "harness".to_string(), into)]
    pub workspace_name: String,

    /// Seed for deterministic graph generation.
    #[builder(default = 1)]
    pub seed: u32,

    /// Prefix used for generated project names (e.g. `bench-p`).
    #[builder(default = "project_".to_string(), into)]
    pub project_prefix: String,

    /// Number of projects to generate.
    #[builder(default = 50)]
    pub projects: usize,

    /// Number of tasks (`t0..t{N-1}`) per project.
    #[builder(default = 3)]
    pub tasks_per_project: usize,

    /// Shape of the inter-project dependency graph.
    #[builder(default)]
    pub dependency: DependencyConfig,

    /// Per-task shape (intra/inter-project deps, command, outputs).
    #[builder(default)]
    pub task: TaskConfig,

    /// The `src/**` input tree each project ships (cache inputs).
    #[builder(default)]
    pub content: ContentConfig,

    /// Whether task/project caching is enabled in the generated config.
    #[builder(default = true)]
    pub cache_enabled: bool,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Inter-project dependency-graph configuration.
#[derive(Debug, Clone, bon::Builder)]
pub struct DependencyConfig {
    /// Shape of the inter-project dependency graph.
    #[builder(default)]
    pub strategy: DependencyStrategy,

    /// Number of layers for the `layered` strategy.
    #[builder(default = 5)]
    pub layers: usize,

    /// Maximum number of upstream dependencies per project (cap for
    /// `layered`/`random`).
    #[builder(default = 3)]
    pub fanout: usize,

    /// Edge inclusion probability for the `random` strategy.
    #[builder(default = 0.35)]
    pub edge_probability: f64,
}

impl Default for DependencyConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Per-task generation shape.
#[derive(Debug, Clone, bon::Builder)]
pub struct TaskConfig {
    /// Whether task `tN` depends on `t(N-1)` within a project.
    #[builder(default = true)]
    pub chain_within_project: bool,

    /// Whether task `tN` depends on `tN` of upstream projects (`^tN`).
    #[builder(default = true)]
    pub fan_upstream: bool,

    /// Number of output files each task writes (and declares) under `dist/`.
    /// Each task runs a small cross-platform launcher script that writes this
    /// many deterministic files, so cold/forced runs exercise output
    /// collection + hashing. `0` still spawns the process but writes nothing.
    #[builder(default = 1)]
    pub output_files: usize,
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// The `src/**` input tree each generated project ships. These files exist so
/// the cache has real inputs to hash; their layout is deterministic.
#[derive(Debug, Clone, bon::Builder)]
pub struct ContentConfig {
    /// Depth of nested folders between the project `src/` root and each leaf.
    #[builder(default = 2)]
    pub folder_nesting: usize,

    /// Number of leaf folders.
    #[builder(default = 5)]
    pub leaf_folder_count: usize,

    /// Number of files per leaf folder.
    #[builder(default = 10)]
    pub files_per_leaf_folder: usize,

    /// File extension for generated content files.
    #[builder(default = "txt".to_string(), into)]
    pub file_extension: String,

    /// Content written to each file. `%i%` is replaced by the file index.
    #[builder(default = "File content %i%".to_string(), into)]
    pub file_content: String,
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}
