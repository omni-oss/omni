//! Pins the zero-work path: `gate_access` on `Ready(target)` touches no
//! registry, session or catalog, and allocates nothing. The crate has no
//! logging dependency and `gate_access` does no I/O, so the panic-on-access
//! fakes below are the evidence for those; allocations are counted per
//! thread so other threads cannot disturb the count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

use omni_migration::*;

struct Counting;

thread_local! {
    static MEASURING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

fn bump() {
    let _ = MEASURING.try_with(|m| {
        if m.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        bump();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bump();
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations_during<T>(f: impl FnOnce() -> T) -> (T, u64) {
    ALLOCATIONS.with(|n| n.set(0));
    MEASURING.with(|m| m.set(true));
    let out = f();
    MEASURING.with(|m| m.set(false));
    (out, ALLOCATIONS.with(Cell::get))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Id;

impl StepId for Id {
    fn stable_id(&self) -> u32 {
        0
    }
}

struct PanicCatalog;

impl Catalog<Id> for PanicCatalog {
    fn id(&self) -> CatalogId {
        panic!("catalog id read")
    }

    fn binding(&self) -> ChainBinding {
        panic!("catalog binding read")
    }

    fn backend(&self) -> BackendSupport {
        panic!("catalog backend read")
    }

    fn capabilities(&self, _: Step<Id>) -> Option<StepCapabilities> {
        panic!("catalog capabilities read")
    }
}

struct PanicSession;

impl Catalog<Id> for PanicSession {
    fn id(&self) -> CatalogId {
        panic!("session catalog read")
    }

    fn binding(&self) -> ChainBinding {
        panic!("session catalog read")
    }

    fn backend(&self) -> BackendSupport {
        panic!("session catalog read")
    }

    fn capabilities(&self, _: Step<Id>) -> Option<StepCapabilities> {
        panic!("session catalog read")
    }
}

impl MigrationSession<Id> for PanicSession {
    fn observe(&self) -> Result<GuardedObservation, Error> {
        panic!("session observation")
    }

    fn probe(&self) -> BackendSupport {
        panic!("session probe")
    }

    fn apply(&mut self, _: AuthorizedStep<Id>) -> StepOutcome {
        panic!("session write")
    }
}

struct PanicStore;

impl MigrationStore<Id> for PanicStore {
    type Session<'a> = PanicSession;

    fn begin_migration(&self) -> Result<PanicSession, Error> {
        panic!("guard acquisition")
    }
}

/// What a cold path would do: open a session and read the catalog.
fn cold(catalog: &PanicCatalog, store: &PanicStore) -> Result<(), Error> {
    let session = store.begin_migration()?;
    let _ = session.observe();
    let _ = catalog.backend();
    Ok(())
}

#[test]
fn ready_at_the_target_does_no_work() {
    let (catalog, store) = (PanicCatalog, PanicStore);
    let target = Version(42);
    let (result, allocations) = allocations_during(|| {
        gate_access(StoreState::Ready(target), target, |_| {
            cold(&catalog, &store)
        })
    });
    assert_eq!(result, Ok(Check::UpToDate));
    assert_eq!(allocations, 0);
}

#[test]
fn recovery_required_is_dirty_without_work() {
    let (catalog, store) = (PanicCatalog, PanicStore);
    let (result, allocations) = allocations_during(|| {
        gate_access(StoreState::RecoveryRequired, Version(42), |_| {
            cold(&catalog, &store)
        })
    });
    assert_eq!(result, Err(Error::Dirty));
    assert_eq!(allocations, 0);
}

#[test]
fn the_fakes_do_panic_when_the_cold_path_is_entered() {
    let (catalog, store) = (PanicCatalog, PanicStore);
    let entered = catch_unwind(AssertUnwindSafe(|| {
        gate_access(StoreState::Ready(Version(7)), Version(42), |_| {
            cold(&catalog, &store)
        })
    }));
    assert!(entered.is_err());
}

#[test]
fn the_allocation_counter_sees_allocations() {
    let (_, allocations) =
        allocations_during(|| std::hint::black_box(vec![1u8]));
    assert!(allocations >= 1);
}
