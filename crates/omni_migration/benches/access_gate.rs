//! Compares the optimized access gate with bare `Version` equality.
//!
//! Inputs are runtime values and every output is consumed through
//! `black_box`, so the compiler cannot fold the comparison away. Adapter I/O
//! is measured separately in `adapter_io`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use omni_migration::{Check, Error, StoreState, Version, gate_access};

fn cold(_: Version) -> Result<(), Error> {
    Err(Error::MissingStep)
}

fn bare_equality(observed: Version, target: Version) -> bool {
    observed == target
}

fn access_gate(c: &mut Criterion) {
    let observed = black_box(Version(42));
    let target = black_box(Version(42));
    let other = black_box(Version(7));
    let mut group = c.benchmark_group("access_gate");

    group.bench_function("bare_version_equality", |b| {
        b.iter(|| {
            black_box(bare_equality(black_box(observed), black_box(target)))
        })
    });
    group.bench_function("gate_access_ready_at_target", |b| {
        b.iter(|| {
            let out = gate_access(
                black_box(StoreState::Ready(observed)),
                black_box(target),
                cold,
            );
            debug_assert!(matches!(out, Ok(Check::UpToDate)));
            black_box(out)
        })
    });
    // A call site that only branches on the result, as ordinary access does.
    group.bench_function("gate_access_ready_at_target_branch_only", |b| {
        b.iter(|| {
            let out = gate_access(
                black_box(StoreState::Ready(observed)),
                black_box(target),
                cold,
            );
            black_box(matches!(out, Ok(Check::UpToDate)))
        })
    });
    group.bench_function("gate_access_ready_at_other_version", |b| {
        b.iter(|| {
            black_box(gate_access(
                black_box(StoreState::Ready(other)),
                black_box(target),
                cold,
            ))
        })
    });
    group.bench_function("gate_access_recovery_required", |b| {
        b.iter(|| {
            black_box(gate_access(
                black_box(StoreState::RecoveryRequired),
                black_box(target),
                cold,
            ))
        })
    });
    group.finish();
}

criterion_group!(benches, access_gate);
criterion_main!(benches);
