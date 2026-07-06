//! Isolated benchmark for the cache-lookup stage
//! (`CacheManager::get_cached_results`): building the store, hashing task
//! inputs into execution info, and reading cached entries. Requires the
//! `bench-support` feature.
//!
//! The cache is primed once (via a real forced execution) in the un-timed
//! setup; only the lookup is measured.

use std::borrow::Cow;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use omni_cache::impls::HybridTaskExecutionCacheStore;
use omni_context::{Context, LoadedContext};
use omni_core::TaskExecutionNode;
use omni_execution_plan::{Call, ExecutionPlanProvider};
use omni_messages::NoopSubscriber;
use omni_task_context::{TaskContext, TaskContextProvider};
use omni_task_executor::{
    ExecutionConfigBuilder, Force, OnFailure, TaskExecutionResult,
    TaskExecutor,
    bench_support::{
        CacheManagerBuilder, CacheStoreProvider, ContextCacheStoreProvider,
        ContextExecutionPlanProvider, DefaultTaskContextProvider,
    },
};
use omni_test_utils::{
    DependencyStrategy, HarnessConfig, generate_workspace, presets::exec_preset,
};
use omni_tracing_subscriber::TracingConfig;
use system_traits::impls::RealSys;
use tempfile::TempDir;
use tokio::runtime::Runtime;

struct Prepared {
    _dir: TempDir,
    ctx: LoadedContext<RealSys>,
    nodes: Vec<TaskExecutionNode>,
    name: String,
}

fn matrix() -> Vec<HarnessConfig> {
    use DependencyStrategy::*;
    vec![exec_preset(120, 3, Layered), exec_preset(120, 3, Chain)]
}

fn prepare(rt: &Runtime) -> Vec<Prepared> {
    let concurrency = num_cpus::get();

    matrix()
        .into_iter()
        .map(|config| {
            let dir = tempfile::tempdir().expect("tempdir");
            generate_workspace(dir.path(), &config).expect("generate");
            let leaf = format!("t{}", config.tasks_per_project - 1);

            let ctx = rt.block_on(async {
                Context::new(
                    RealSys,
                    "development",
                    dir.path(),
                    false,
                    "workspace.omni.yaml",
                    None,
                    &TracingConfig::disabled(),
                )
                .expect("context")
                .into_loaded()
                .await
                .expect("load")
            });

            // Prime the cache with a real forced execution.
            let cfg = ExecutionConfigBuilder::default()
                .on_failure(OnFailure::Continue)
                .max_concurrency(concurrency)
                .force(Force::All)
                .call(Call::new_tasks(vec![leaf.clone()]))
                .build()
                .expect("config");
            rt.block_on(TaskExecutor::new(cfg, &ctx, NoopSubscriber).run())
                .expect("prime run");

            // Materialize the task-graph nodes for the called task.
            let plan = ContextExecutionPlanProvider::new(&ctx)
                .get_execution_plan(
                    &Call::new_tasks(vec![leaf.clone()]),
                    &[],
                    &[],
                    None,
                    None,
                    false,
                    false,
                )
                .expect("plan");
            let nodes: Vec<TaskExecutionNode> =
                plan.iter().flatten().cloned().collect();

            Prepared {
                _dir: dir,
                ctx,
                nodes,
                name: config.workspace_name,
            }
        })
        .collect()
}

fn benches(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime");
    let prepared = prepare(&rt);

    let mut g = c.benchmark_group("pipeline/cache_lookup");
    for p in &prepared {
        let overall: maps::UnorderedMap<String, TaskExecutionResult> =
            maps::unordered_map!();
        let provider = DefaultTaskContextProvider::new(&p.ctx, &overall, None);
        let contexts: Vec<Cow<TaskContext>> = p
            .nodes
            .iter()
            .map(|n| {
                Cow::Owned(
                    provider.get_task_context(n, false).expect("task context"),
                )
            })
            .collect();

        let store = ContextCacheStoreProvider::new(&p.ctx).get_cache_store();
        let cm = CacheManagerBuilder::<HybridTaskExecutionCacheStore, RealSys>::default()
            .store(store)
            .sys(p.ctx.sys().clone())
            .root_dir(p.ctx.root_dir().to_path_buf())
            .cache_dir(p.ctx.cache_dir())
            .force(Force::None)
            .build()
            .expect("cache manager");

        g.bench_with_input(
            BenchmarkId::from_parameter(&p.name),
            &(),
            |b, _| {
                b.to_async(&rt).iter(|| cm.get_cached_results(&contexts));
            },
        );
    }
    g.finish();
}

criterion_group!(
    name = cache_lookup;
    config = Criterion::default();
    targets = benches
);
criterion_main!(cache_lookup);
