//! Harness primitives: workspace setup, execution scenarios, and the executor
//! driver. Everything here is designed so the criterion benches keep generation
//! and context loading out of the timed region.

use std::path::{Path, PathBuf};

use omni_context::{Context, LoadedContext};
use omni_messages::NoopSubscriber;
use omni_task_executor::{
    Call, ExecutionConfig, ExecutionConfigBuilder, Force, OnFailure,
    TaskExecutionResult, TaskExecutor,
};
use omni_test_utils::{HarnessConfig, generate_workspace};
use omni_tracing_subscriber::TracingConfig;
use system_traits::impls::RealSys;
use tempfile::TempDir;

/// A generated benchmark workspace on disk. The `TempDir` is retained so the
/// directory lives as long as the workspace is in use and is cleaned up on
/// drop. Construct with [`BenchWorkspace::generate`] — this is **not** timed by
/// the benches.
pub struct BenchWorkspace {
    _dir: TempDir,
    /// Absolute path to the generated workspace root.
    pub root: PathBuf,
    /// Number of task-graph nodes (projects × tasks-per-project). Used as the
    /// criterion throughput so results read as per-node cost.
    pub node_count: u64,
}

impl BenchWorkspace {
    /// Generate a workspace for `config` in a fresh temp dir.
    pub fn generate(config: &HarnessConfig) -> eyre::Result<Self> {
        let dir = tempfile::tempdir()?;
        let nodes = generate_workspace(dir.path(), config)?;
        let node_count = (nodes.len() * config.tasks_per_project) as u64;
        Ok(Self {
            root: dir.path().to_path_buf(),
            _dir: dir,
            node_count,
        })
    }
}

/// Build a `LoadedContext` for a generated workspace. Not timed by the benches
/// (except the dedicated `load_and_run` group). Tracing is disabled and env
/// inheritance is off so the measured region reflects omni's own overhead.
pub async fn build_loaded_context(root: &Path) -> LoadedContext<RealSys> {
    let ctx = Context::new(
        RealSys,
        "development",
        root,
        false,
        "workspace.omni.yaml",
        None,
        &TracingConfig::disabled(),
    )
    .expect("failed to create context");

    ctx.into_loaded().await.expect("failed to load context")
}

/// A cache-state scenario. Each maps to a distinct code path in the executor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Cache primed; every task is a cache hit (replay path). The primary
    /// per-invocation overhead metric — read-only, so safe to iterate.
    Warm,
    /// `Force::All`: skip the cache read and re-run every process; cache is
    /// re-written each iteration (deterministic overwrite).
    Cold,
    /// `Force::All` + `no_cache`: re-run processes without writing the cache.
    NoCache,
    /// `dry_run`: no process exec and no cache write, but the collector still
    /// computes digests — isolates hashing/collection cost.
    DryRun,
}

impl Scenario {
    /// Build the `ExecutionConfig` for this scenario.
    pub fn config(
        self,
        tasks: Vec<String>,
        concurrency: usize,
    ) -> ExecutionConfig {
        let mut b = ExecutionConfigBuilder::default();
        b.on_failure(OnFailure::Continue)
            .ignore_dependencies(false)
            .with_dependents(false)
            .max_concurrency(concurrency)
            .call(Call::new_tasks(tasks));

        match self {
            Scenario::Warm => {
                b.force(Force::None).no_cache(false).dry_run(false);
            }
            Scenario::Cold => {
                b.force(Force::All).no_cache(false).dry_run(false);
            }
            Scenario::NoCache => {
                b.force(Force::All).no_cache(true).dry_run(false);
            }
            Scenario::DryRun => {
                b.force(Force::None).no_cache(false).dry_run(true);
            }
        }

        b.build().expect("failed to build execution config")
    }
}

/// Run the executor for `tasks` under `scenario`. This is the timed operation.
pub async fn run_executor(
    ctx: &LoadedContext<RealSys>,
    tasks: Vec<String>,
    scenario: Scenario,
    concurrency: usize,
) -> Vec<TaskExecutionResult> {
    let config = scenario.config(tasks, concurrency);
    let executor = TaskExecutor::new(config, ctx, NoopSubscriber);
    executor.run().await.expect("executor run failed")
}

/// Prime the cache (one un-timed cold run) and assert the subsequent warm run
/// is 100% cache hits. This is the in-process analogue of `task-bench`'s
/// exec-log verification: it fails loudly if the "warm" measurement would
/// silently be measuring the cold path.
pub async fn prime_and_verify_warm(
    ctx: &LoadedContext<RealSys>,
    tasks: Vec<String>,
    concurrency: usize,
) {
    // Populate the cache by forcing a full execution once.
    let _ = run_executor(ctx, tasks.clone(), Scenario::Cold, concurrency).await;

    // Now a warm run must be all cache hits.
    let results = run_executor(ctx, tasks, Scenario::Warm, concurrency).await;

    let total = results.len();
    let hits = results
        .iter()
        .filter(|r| {
            matches!(
                r,
                TaskExecutionResult::Completed {
                    cache_hit: true,
                    ..
                }
            )
        })
        .count();

    assert!(
        total > 0 && hits == total,
        "warm run must be 100% cache hits (got {hits}/{total}); \
         the benchmark would otherwise measure the cold path"
    );
}
