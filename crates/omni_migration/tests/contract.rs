//! Storage-free contract tests: they use an in-memory adapter and run on
//! every platform.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use omni_migration::*;

const V1: Version = Version(1);
const V2: Version = Version(5);
const V3: Version = Version(3);
const MEM: ChainName = ChainName::new("mem");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S {
    Up1,
    Up2,
    Down2,
    Down1,
}

impl StepId for S {
    fn stable_id(&self) -> u32 {
        match self {
            S::Up1 => 5,
            S::Up2 => 3,
            S::Down2 => 9,
            S::Down1 => 1,
        }
    }
}

const EDGES: &[Transition<S>] = &[
    Transition {
        previous: V1,
        version: V2,
        upgrade: S::Up1,
        downgrade: Some(S::Down1),
    },
    Transition {
        previous: V2,
        version: V3,
        upgrade: S::Up2,
        downgrade: Some(S::Down2),
    },
];

fn chain() -> Chain<'static, S> {
    Chain::new(V1, EDGES).unwrap()
}

struct State {
    store: u64,
    version: Version,
    revision: u64,
    intent: bool,
}

type Hook = Box<dyn FnMut(&mut State) + Send>;

#[derive(Default)]
struct Hooks {
    after_apply: Option<Hook>,
}

struct Mem {
    state: Mutex<State>,
    hooks: Mutex<Hooks>,
    applied: AtomicUsize,
}

impl Mem {
    fn new() -> Self {
        Self {
            state: Mutex::new(State {
                store: 7,
                version: V1,
                revision: 1,
                intent: false,
            }),
            hooks: Mutex::default(),
            applied: AtomicUsize::new(0),
        }
    }

    fn after_apply(&self, f: impl FnMut(&mut State) + Send + 'static) {
        self.hooks.lock().unwrap().after_apply = Some(Box::new(f));
    }

    fn observe(&self) -> GuardedObservation {
        let s = self.state.lock().unwrap();
        if s.intent {
            return GuardedObservation::RecoveryRequired;
        }
        GuardedObservation::Ready(Stamp {
            store: StoreId(s.store),
            chain: MEM,
            revision: Revision(s.revision),
            version: s.version,
        })
    }

    fn plan(&self, to: Version, d: Direction) -> Result<Plan<S>, Error> {
        let GuardedObservation::Ready(stamp) = self.observe() else {
            return Err(Error::Dirty);
        };
        build_plan(&chain(), &MemCatalog, stamp, to, d, Policy::default())
    }

    fn version(&self) -> Version {
        self.state.lock().unwrap().version
    }
}

struct MemCatalog;

impl Catalog<S> for MemCatalog {
    fn id(&self) -> CatalogId {
        CatalogId(9)
    }

    fn binding(&self) -> ChainBinding {
        chain().binding(MEM)
    }

    fn backend(&self) -> BackendSupport {
        BackendSupport::FULL
    }

    fn capabilities(&self, _: Step<S>) -> Option<StepCapabilities> {
        Some(StepCapabilities {
            atomic: true,
            lossless: true,
            derivable: false,
        })
    }
}

struct MemSession<'a>(&'a Mem);

impl Catalog<S> for MemSession<'_> {
    fn id(&self) -> CatalogId {
        MemCatalog.id()
    }

    fn binding(&self) -> ChainBinding {
        MemCatalog.binding()
    }

    fn backend(&self) -> BackendSupport {
        MemCatalog.backend()
    }

    fn capabilities(&self, s: Step<S>) -> Option<StepCapabilities> {
        MemCatalog.capabilities(s)
    }
}

impl MigrationSession<S> for MemSession<'_> {
    fn observe(&self) -> Result<GuardedObservation, Error> {
        Ok(self.0.observe())
    }

    fn probe(&self) -> BackendSupport {
        BackendSupport::FULL
    }

    fn apply(&mut self, step: AuthorizedStep<S>) -> StepOutcome {
        let step = step.step();
        let mut s = self.0.state.lock().unwrap();
        if s.version != step.from {
            return StepOutcome::NotCommitted(Error::UnexpectedVersion);
        }
        s.version = step.to;
        s.revision += 1;
        self.0.applied.fetch_add(1, Ordering::SeqCst);
        if let Some(f) = self.0.hooks.lock().unwrap().after_apply.as_mut() {
            f(&mut s);
        }
        StepOutcome::Committed
    }
}

impl MigrationStore<S> for Mem {
    type Session<'a> = MemSession<'a>;

    fn begin_migration(&self) -> Result<MemSession<'_>, Error> {
        Ok(MemSession(self))
    }
}

#[test]
fn a_clean_run_commits_every_step() {
    let m = Mem::new();
    let plan = m.plan(V3, Direction::Upgrade).unwrap();
    assert_eq!(plan.steps().len(), 2);
    assert_eq!(execute(&m, &plan, || false), Ok(Run::Finished(V3)));
    let back = m.plan(V1, Direction::Downgrade).unwrap();
    assert_eq!(execute(&m, &back, || false), Ok(Run::Finished(V1)));
}

#[test]
fn the_runner_reobserves_between_steps() {
    type Case = fn(&mut State);
    let cases: [(&str, Case, Error); 4] = [
        ("intent appears", |s| s.intent = true, Error::Dirty),
        ("store replaced", |s| s.store += 1, Error::StalePlan),
        (
            "peer moves the version",
            |s| s.version = V1,
            Error::StalePlan,
        ),
        (
            "peer moves past the plan",
            |s| s.version = V3,
            Error::StalePlan,
        ),
    ];
    for (name, hook, error) in cases {
        let m = Mem::new();
        let plan = m.plan(V3, Direction::Upgrade).unwrap();
        m.after_apply(hook);
        assert_eq!(
            execute(&m, &plan, || false),
            Ok(Run::Stopped { last: V2, error }),
            "{name}"
        );
        assert_eq!(m.applied.load(Ordering::SeqCst), 1, "{name}");
    }
}

#[test]
fn a_revision_bump_between_steps_is_adapter_defined_and_allowed() {
    let m = Mem::new();
    let plan = m.plan(V3, Direction::Upgrade).unwrap();
    assert_eq!(execute(&m, &plan, || false), Ok(Run::Finished(V3)));
    assert_eq!(m.version(), V3);
}

#[test]
fn a_stale_revision_is_rejected_before_any_step() {
    let m = Mem::new();
    let plan = m.plan(V3, Direction::Upgrade).unwrap();
    m.state.lock().unwrap().revision += 1;
    assert_eq!(execute(&m, &plan, || false), Err(Error::StalePlan));
    assert_eq!(m.applied.load(Ordering::SeqCst), 0);
}

fn hex(d: [u8; 32]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_chain_digest_is_stable_and_covers_every_input() {
    // Pins the byte encoding: a change here changes every recorded binding.
    assert_eq!(
        hex(chain().binding(MEM).digest),
        "5cc3d8fff114b6c57ab7b99c0306ad271e9fb5f1257e33800e690ffa07751c51"
    );
    let mut bytes = b"omni_migration.chain.v1\0".to_vec();
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(2u64.to_le_bytes());
    for (previous, version, up, down) in
        [(1u32, 5u32, 5u32, 1u32), (5, 3, 3, 9)]
    {
        for n in [previous, version, up] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.push(1);
        bytes.extend(down.to_le_bytes());
    }
    assert_eq!(
        chain().binding(MEM).digest,
        *blake3::hash(&bytes).as_bytes()
    );
    let base = chain().binding(MEM);
    assert_eq!(base, chain().binding(MEM));

    let mut changed = EDGES.to_vec();
    changed[1].upgrade = S::Down2;
    changed[1].downgrade = Some(S::Up2);
    let mut no_down = EDGES.to_vec();
    no_down[1].downgrade = None;
    let swapped_versions = [
        Transition {
            previous: V1,
            version: V3,
            upgrade: S::Up1,
            downgrade: Some(S::Down1),
        },
        Transition {
            previous: V3,
            version: V2,
            upgrade: S::Up2,
            downgrade: Some(S::Down2),
        },
    ];
    for edges in [
        &changed[..],
        &no_down[..],
        &swapped_versions[..],
        &EDGES[..1],
    ] {
        let other = Chain::new(V1, edges).unwrap().binding(MEM);
        assert_ne!(other.digest, base.digest);
    }
    assert_ne!(
        Chain::<S>::new(V2, &[]).unwrap().binding(MEM).digest,
        base.digest
    );
    // The name is part of the binding, not of the digest.
    let named = chain().binding(ChainName::new("other"));
    assert_eq!(named.digest, base.digest);
    assert_ne!(named, base);
}
