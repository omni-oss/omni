use std::hint::black_box;
use std::path::Path;

use criterion::{BenchmarkId, criterion_group, criterion_main};
use omni_context::Context;
use omni_test_utils::presets;
use omni_tracing_subscriber::TracingConfig;
use system_traits::impls::RealSys;

async fn load_projects(ws_dir: &Path) {
    let ctx = Context::new(
        RealSys,
        "development",
        ws_dir,
        false,
        "workspace.omni.yaml",
        Some(vec![".env".into()]),
        &TracingConfig::default(),
    )
    .expect("can't create context");

    ctx.into_loaded().await.expect("can't load projects");
}

fn load_projects_benchmarks(c: &mut criterion::Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("Can't create runtime");
    let mut group = c.benchmark_group("load_projects");

    for preset in [&presets::JS_SMALL, &presets::JS_MEDIUM, &presets::JS_LARGE]
    {
        let dir = tempfile::tempdir().expect("Can't create temp dir");

        presets::generate(dir.path(), preset).expect("Can't generate projects");

        group.bench_with_input(
            BenchmarkId::new("load_projects", preset.workspace_name.as_str()),
            dir.path(),
            |b, ws_dir| {
                b.to_async(&rt).iter(|| load_projects(black_box(ws_dir)))
            },
        );
    }
}

fn criterion_config() -> criterion::Criterion {
    criterion::Criterion::default()
}

criterion_group!(
    name = load_projects_benches;
    config = criterion_config();
    targets = load_projects_benchmarks
);

criterion_main!(load_projects_benches);
