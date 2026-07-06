//! End-to-end task-execution benchmarks.
//!
//! Measures [`omni_task_executor::TaskExecutor::run`] over a varied matrix
//! (scale × shape × density) under several cache-state scenarios. Workspace
//! generation, context loading, and cache priming happen in the (un-timed)
//! setup; only `run()` is timed. Throughput is reported per task-graph node.

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use omni_bench::harness::{
    BenchWorkspace, Scenario, build_loaded_context, prime_and_verify_warm,
    run_executor,
};
use omni_bench::matrix::{concurrency_values, default_matrix, heavy_subset};
use omni_context::LoadedContext;
use system_traits::impls::RealSys;
use tokio::runtime::Runtime;

/// A prepared, primed workspace ready to be benchmarked. Holds the `TempDir`
/// (via `ws`) so it lives for the whole run.
struct Prepared {
    ws: BenchWorkspace,
    ctx: LoadedContext<RealSys>,
    name: String,
    leaf: String,
}

fn prepare(rt: &Runtime, concurrency: usize) -> Vec<Prepared> {
    default_matrix()
        .into_iter()
        .map(|entry| {
            let ws = BenchWorkspace::generate(&entry.config)
                .expect("generate workspace");
            let ctx = rt.block_on(build_loaded_context(&ws.root));
            let leaf = entry.leaf_task();
            rt.block_on(prime_and_verify_warm(
                &ctx,
                vec![leaf.clone()],
                concurrency,
            ));
            Prepared {
                ws,
                ctx,
                name: entry.name,
                leaf,
            }
        })
        .collect()
}

fn scenario_group(
    c: &mut Criterion,
    rt: &Runtime,
    prepared: &[Prepared],
    group_name: &str,
    scenario: Scenario,
    concurrency: usize,
) {
    let mut g = c.benchmark_group(group_name);
    for p in prepared {
        g.throughput(Throughput::Elements(p.ws.node_count));
        g.bench_with_input(BenchmarkId::from_parameter(&p.name), p, |b, p| {
            b.to_async(rt).iter(|| {
                run_executor(
                    &p.ctx,
                    vec![p.leaf.clone()],
                    scenario,
                    concurrency,
                )
            });
        });
    }
    g.finish();
}

fn heavy_scenario_group(
    c: &mut Criterion,
    rt: &Runtime,
    heavy: &[&Prepared],
    group_name: &str,
    scenario: Scenario,
    concurrency: usize,
) {
    let mut g = c.benchmark_group(group_name);
    g.sample_size(10);
    for p in heavy {
        g.throughput(Throughput::Elements(p.ws.node_count));
        g.bench_with_input(BenchmarkId::from_parameter(&p.name), p, |b, p| {
            b.to_async(rt).iter(|| {
                run_executor(
                    &p.ctx,
                    vec![p.leaf.clone()],
                    scenario,
                    concurrency,
                )
            });
        });
    }
    g.finish();
}

fn benches(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let concurrency = num_cpus::get();

    let prepared = prepare(&rt, concurrency);

    // Warm (all-cache-hit): the primary per-invocation overhead metric.
    scenario_group(
        c,
        &rt,
        &prepared,
        "task_execution/warm",
        Scenario::Warm,
        concurrency,
    );

    // Dry-run: isolates hashing/collection (no exec, no cache write).
    scenario_group(
        c,
        &rt,
        &prepared,
        "task_execution/dry_run",
        Scenario::DryRun,
        concurrency,
    );

    // Cold (forced exec) + no-cache: only for a small representative subset,
    // since each iteration spawns real processes. Reduced sample size.
    let heavy_names: Vec<String> =
        heavy_subset().into_iter().map(|e| e.name).collect();
    let heavy: Vec<&Prepared> = prepared
        .iter()
        .filter(|p| heavy_names.contains(&p.name))
        .collect();

    heavy_scenario_group(
        c,
        &rt,
        &heavy,
        "task_execution/cold",
        Scenario::Cold,
        concurrency,
    );
    heavy_scenario_group(
        c,
        &rt,
        &heavy,
        "task_execution/no_cache",
        Scenario::NoCache,
        concurrency,
    );

    // Concurrency sweep (warm) on one representative config: scheduler /
    // semaphore / tokio-task overhead as parallelism changes.
    if let Some(p) = prepared.iter().find(|p| p.name.contains("layered_120")) {
        let mut g = c.benchmark_group("task_execution/concurrency");
        for conc in concurrency_values() {
            g.throughput(Throughput::Elements(p.ws.node_count));
            g.bench_with_input(
                BenchmarkId::from_parameter(conc),
                &conc,
                |b, &conc| {
                    b.to_async(&rt).iter(|| {
                        run_executor(
                            &p.ctx,
                            vec![p.leaf.clone()],
                            Scenario::Warm,
                            conc,
                        )
                    });
                },
            );
        }
        g.finish();
    }

    // True end-to-end: discovery + context load + warm run, per invocation.
    {
        let mut g = c.benchmark_group("task_execution/load_and_run");
        g.sample_size(10);
        for p in prepared.iter().filter(|p| heavy_names.contains(&p.name)) {
            g.throughput(Throughput::Elements(p.ws.node_count));
            g.bench_with_input(
                BenchmarkId::from_parameter(&p.name),
                p,
                |b, p| {
                    b.to_async(&rt).iter(|| async {
                        let ctx = build_loaded_context(&p.ws.root).await;
                        run_executor(
                            &ctx,
                            vec![p.leaf.clone()],
                            Scenario::Warm,
                            concurrency,
                        )
                        .await
                    });
                },
            );
        }
        g.finish();
    }
}

criterion_group!(
    name = benches_group;
    config = Criterion::default();
    targets = benches
);
criterion_main!(benches_group);
