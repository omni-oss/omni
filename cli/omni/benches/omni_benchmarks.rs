use criterion::{criterion_group, criterion_main};

fn run_benchmarks(_c: &mut criterion::Criterion) {}

fn criterion_config() -> criterion::Criterion {
    criterion::Criterion::default()
}

criterion_group!(
    name = run_benches;
    config = criterion_config();
    targets = run_benchmarks
);

criterion_main!(run_benches);
