use criterion::{Criterion, criterion_group, criterion_main};
use env::{EnvParseError, ParseConfig};
use maps::Map;
use std::hint::black_box;

const LARGE_TEST_CONTENT: &str = include_str!("./fixtures/5000.env");
const MEDIUM_TEST_CONTENT: &str = include_str!("./fixtures/500.env");
const TRIVIAL_TEST_CONTENT: &str = include_str!("./fixtures/trivial.env");

fn parse(text: &str) -> Result<Map<String, String>, EnvParseError> {
    env::parse(text, &ParseConfig::default())
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    group.bench_with_input("trivial", TRIVIAL_TEST_CONTENT, |b, input| {
        b.iter(|| parse(black_box(input)))
    });

    group.bench_with_input("medium", MEDIUM_TEST_CONTENT, |b, input| {
        b.iter(|| parse(black_box(input)))
    });

    group.bench_with_input("large", LARGE_TEST_CONTENT, |b, input| {
        b.iter(|| parse(black_box(input)))
    });
    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
}

criterion_group!(
    name = benches;
    config = criterion_config();
    targets = criterion_benchmark
);

criterion_main!(benches);
