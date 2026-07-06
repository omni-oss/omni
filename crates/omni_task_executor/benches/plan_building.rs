//! Isolated benchmark for the execution-plan-building stage
//! (`get_execution_plan`): project-graph construction, filtering, and
//! topological batching. Requires the `bench-support` feature.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use omni_context::{Context, LoadedContext};
use omni_execution_plan::{Call, ExecutionPlanProvider};
use omni_task_executor::bench_support::ContextExecutionPlanProvider;
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
    name: String,
    leaf: String,
}

fn matrix() -> Vec<HarnessConfig> {
    use DependencyStrategy::*;
    vec![
        exec_preset(120, 3, Layered),
        exec_preset(120, 3, Chain),
        exec_preset(120, 3, FanOut),
    ]
}

fn prepare(rt: &Runtime) -> Vec<Prepared> {
    matrix()
        .into_iter()
        .map(|config| {
            let dir = tempfile::tempdir().expect("tempdir");
            generate_workspace(dir.path(), &config).expect("generate");
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
            let leaf = format!("t{}", config.tasks_per_project - 1);
            Prepared {
                _dir: dir,
                ctx,
                name: config.workspace_name,
                leaf,
            }
        })
        .collect()
}

fn benches(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime");
    let prepared = prepare(&rt);

    let mut g = c.benchmark_group("pipeline/plan_building");
    for p in &prepared {
        let provider = ContextExecutionPlanProvider::new(&p.ctx);
        let call = Call::new_tasks(vec![p.leaf.clone()]);
        g.bench_with_input(
            BenchmarkId::from_parameter(&p.name),
            &(),
            |b, _| {
                b.iter(|| {
                    let plan = provider
                        .get_execution_plan(
                            &call,
                            &[],
                            &[],
                            None,
                            None,
                            false,
                            false,
                        )
                        .expect("plan");
                    black_box(plan);
                });
            },
        );
    }
    g.finish();
}

criterion_group!(
    name = plan_building;
    config = Criterion::default();
    targets = benches
);
criterion_main!(plan_building);
