//! Cost of the reference fixtures' guarded observation, which does real file
//! I/O. Reported separately from the zero-work gate, which does none.

#[cfg(unix)]
#[path = "../tests/common/mod.rs"]
mod common;

#[cfg(unix)]
use std::hint::black_box;

#[cfg(unix)]
use common::fs::Faults;
#[cfg(unix)]
use common::index::{IndexStore, WITH_INDEX};
#[cfg(unix)]
use common::lockfile::PinsCatalog;
#[cfg(unix)]
use common::v1_store;
#[cfg(unix)]
use criterion::{Criterion, criterion_group, criterion_main};
#[cfg(unix)]
use omni_migration::{MigrationSession, MigrationStore};

#[cfg(unix)]
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

#[cfg(unix)]
criterion_group!(benches, adapter_io);
#[cfg(unix)]
criterion_main!(benches);

// The fixtures fsync directories, which Windows does not support.
#[cfg(not(unix))]
fn main() {}
