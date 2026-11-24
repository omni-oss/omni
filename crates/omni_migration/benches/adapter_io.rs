//! Cost of the reference fixtures' guarded observation, which does real file
//! I/O. Reported separately from the zero-work gate, which does none.

#[path = "../tests/common/mod.rs"]
mod common;

use std::hint::black_box;

use common::fs::Faults;
use common::index::{IndexStore, WITH_INDEX};
use common::lockfile::PinsCatalog;
use common::v1_store;
use criterion::{Criterion, criterion_group, criterion_main};
use omni_migration::{MigrationSession, MigrationStore};

fn adapter_io(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = v1_store(dir.path(), PinsCatalog::two());
    let dir2 = tempfile::tempdir().unwrap();
    let index = IndexStore::create(
        dir2.path(),
        Faults::new(),
        1,
        WITH_INDEX,
        &["a", "bb", "ccc"],
    )
    .unwrap();
    let mut group = c.benchmark_group("adapter_io");

    group.bench_function("lockfile_observe", |b| {
        b.iter(|| black_box(lockfile.observe()))
    });
    group.bench_function("index_observe", |b| {
        b.iter(|| black_box(index.observe()))
    });
    group.bench_function("lockfile_begin_migration_and_observe", |b| {
        b.iter(|| {
            let session = lockfile.begin_migration().unwrap();
            black_box(session.observe())
        })
    });
    group.finish();
}

criterion_group!(benches, adapter_io);
criterion_main!(benches);
