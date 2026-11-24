use std::collections::HashSet;
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Version(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StoreId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Revision(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CatalogId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChainName(pub &'static str);

impl ChainName {
    pub const PINS: Self = Self("pins");
    pub const INDEX: Self = Self("index");
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainBinding {
    pub name: ChainName,
    pub digest: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(String),
    #[error("missing version")]
    MissingVersion,
    #[error("malformed version")]
    MalformedVersion,
    #[error("unknown but well-formed version tag")]
    UnknownTag,
    #[error("unknown version {0:?}")]
    UnknownVersion(Version),
    #[error("missing chain name")]
    MissingChain,
    #[error("malformed chain name")]
    MalformedChain,
    #[error("store belongs to a different chain")]
    ChainMismatch,
    #[error("catalog or chain differs from the plan")]
    StaleCatalog,
    #[error("store changed since the plan")]
    StalePlan,
    #[error("invalid chain")]
    InvalidChain,
    #[error("step not implemented")]
    MissingStep,
    #[error("no downgrade implementation on the reverse path")]
    NoDowngradePath,
    #[error("requested direction does not match the target")]
    WrongDirection,
    #[error("non-atomic step not allowed")]
    NonAtomic,
    #[error("derivable loss not allowed")]
    DerivableLoss,
    #[error("irreversible loss not allowed")]
    IrreversibleLoss,
    #[error("backend cannot honor the step")]
    UnsupportedBackend,
    #[error("store needs recovery")]
    Dirty,
    #[error("cancelled")]
    Cancelled,
    #[error("unexpected version during step")]
    UnexpectedVersion,
    #[error("downgrade would drop meaningful notes")]
    MeaningfulNotes,
    #[error("downgrade would drop meaningful tags")]
    MeaningfulTags,
    #[error("malformed document: {0}")]
    MalformedDocument(String),
    #[error("malformed intent")]
    MalformedIntent,
    #[error("inconsistent layout: {0}")]
    Inconsistent(String),
    #[error("recovery evidence does not match any known state")]
    RecoveryBlocked,
    #[error("another process holds the migration guard")]
    GuardBusy,
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(format!("{:?}: {e}", e.kind()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreState {
    Ready(Version),
    RecoveryRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub store: StoreId,
    pub chain: ChainName,
    pub revision: Revision,
    pub version: Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepCapabilities {
    pub atomic: bool,
    pub lossless: bool,
    pub derivable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendSupport {
    pub durable: bool,
    pub atomic_publish: bool,
}

impl BackendSupport {
    pub const FULL: Self = Self {
        durable: true,
        atomic_publish: true,
    };
    pub const DURABLE_NON_ATOMIC: Self = Self {
        durable: true,
        atomic_publish: false,
    };
    pub const NONE: Self = Self {
        durable: false,
        atomic_publish: false,
    };
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Policy {
    pub allow_non_atomic: bool,
    pub allow_derivable_loss: bool,
    pub allow_irreversible_loss: bool,
}

impl Policy {
    pub fn check(self, c: StepCapabilities) -> Result<(), Error> {
        if !c.atomic && !self.allow_non_atomic {
            return Err(Error::NonAtomic);
        }
        match (c.lossless, c.derivable) {
            (true, _) => Ok(()),
            (false, true) if self.allow_derivable_loss => Ok(()),
            (false, false) if self.allow_irreversible_loss => Ok(()),
            (false, true) => Err(Error::DerivableLoss),
            (false, false) => Err(Error::IrreversibleLoss),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Upgrade,
    Downgrade,
}

#[derive(Clone, Copy, Debug)]
pub struct Transition<StepId> {
    pub previous: Version,
    pub version: Version,
    pub upgrade: StepId,
    pub downgrade: Option<StepId>,
}

pub struct Chain<'a, StepId> {
    pub base: Version,
    pub edges: &'a [Transition<StepId>],
}

impl<'a, StepId: Copy + Eq + Hash> Chain<'a, StepId> {
    pub fn new(
        base: Version,
        edges: &'a [Transition<StepId>],
    ) -> Result<Self, Error> {
        let mut versions = HashSet::from([base]);
        let mut ids = HashSet::new();
        let mut previous = base;
        for edge in edges {
            if edge.previous != previous
                || !versions.insert(edge.version)
                || !ids.insert(edge.upgrade)
                || edge.downgrade.is_some_and(|id| !ids.insert(id))
            {
                return Err(Error::InvalidChain);
            }
            previous = edge.version;
        }
        Ok(Self { base, edges })
    }

    pub fn binding(&self, name: ChainName) -> ChainBinding {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.base.hash(&mut h);
        for e in self.edges {
            e.previous.hash(&mut h);
            e.version.hash(&mut h);
            e.upgrade.hash(&mut h);
            e.downgrade.hash(&mut h);
        }
        ChainBinding {
            name,
            digest: h.finish(),
        }
    }

    pub fn index(&self, v: Version) -> Result<usize, Error> {
        if v == self.base {
            return Ok(0);
        }
        self.edges
            .iter()
            .position(|e| e.version == v)
            .map(|i| i + 1)
            .ok_or(Error::UnknownVersion(v))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step<StepId> {
    pub from: Version,
    pub to: Version,
    pub id: StepId,
}

#[derive(Debug)]
pub struct Plan<StepId> {
    pub observed: Stamp,
    pub catalog: CatalogId,
    pub binding: ChainBinding,
    pub direction: Direction,
    pub target: Version,
    pub policy: Policy,
    pub steps: Vec<(Step<StepId>, StepCapabilities)>,
}

pub trait Catalog<StepId> {
    fn id(&self) -> CatalogId;
    fn binding(&self) -> ChainBinding;
    fn backend(&self) -> BackendSupport;
    fn capabilities(&self, step: Step<StepId>) -> Option<StepCapabilities>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Check<T> {
    UpToDate,
    Work(T),
}

pub fn gate_access<T>(
    observed: StoreState,
    target: Version,
    cold: impl FnOnce(Version) -> Result<T, Error>,
) -> Result<Check<T>, Error> {
    match observed {
        StoreState::Ready(v) if v == target => Ok(Check::UpToDate),
        StoreState::Ready(v) => cold(v).map(Check::Work),
        StoreState::RecoveryRequired => Err(Error::Dirty),
    }
}

pub fn check_backend(
    b: BackendSupport,
    c: StepCapabilities,
) -> Result<(), Error> {
    if !b.durable {
        return Err(Error::UnsupportedBackend);
    }
    if c.atomic && !b.atomic_publish {
        return Err(Error::UnsupportedBackend);
    }
    Ok(())
}

pub fn build_plan<StepId: Copy + Eq + Hash>(
    chain: &Chain<'_, StepId>,
    catalog: &impl Catalog<StepId>,
    observed: Stamp,
    target: Version,
    direction: Direction,
    policy: Policy,
) -> Result<Plan<StepId>, Error> {
    if observed.chain != catalog.binding().name {
        return Err(Error::ChainMismatch);
    }
    if chain.binding(observed.chain) != catalog.binding() {
        return Err(Error::StaleCatalog);
    }
    let (a, b) = (chain.index(observed.version)?, chain.index(target)?);
    let mut steps = Vec::new();
    let mut push = |step| {
        let c = catalog.capabilities(step).ok_or(Error::MissingStep)?;
        check_backend(catalog.backend(), c)?;
        policy.check(c)?;
        steps.push((step, c));
        Ok::<_, Error>(())
    };
    match direction {
        Direction::Upgrade if a < b => {
            for e in &chain.edges[a..b] {
                push(Step {
                    from: e.previous,
                    to: e.version,
                    id: e.upgrade,
                })?;
            }
        }
        Direction::Downgrade if b < a => {
            for e in chain.edges[b..a].iter().rev() {
                push(Step {
                    from: e.version,
                    to: e.previous,
                    id: e.downgrade.ok_or(Error::NoDowngradePath)?,
                })?;
            }
        }
        _ => return Err(Error::WrongDirection),
    }
    Ok(Plan {
        observed,
        catalog: catalog.id(),
        binding: catalog.binding(),
        direction,
        target,
        policy,
        steps,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum StepOutcome {
    Committed,
    NotCommitted(Error),
    RecoveryRequired(Error),
}

#[derive(Debug, PartialEq, Eq)]
pub enum GuardedObservation {
    Ready(Stamp),
    RecoveryRequired,
}

pub trait MigrationStore<StepId> {
    type Session<'a>: MigrationSession<StepId>
    where
        Self: 'a;
    fn begin_migration(&self) -> Result<Self::Session<'_>, Error>;
}

pub trait MigrationSession<StepId>: Catalog<StepId> {
    fn observe(&self) -> Result<GuardedObservation, Error>;
    fn probe(&self) -> BackendSupport;
    fn apply(&mut self, step: Step<StepId>) -> StepOutcome;
}

#[derive(Debug, PartialEq, Eq)]
pub enum Run {
    Finished(Version),
    Stopped {
        last: Version,
        error: Error,
    },
    NeedsRecovery {
        last_confirmed: Version,
        attempted: Version,
        error: Error,
    },
}

pub fn execute<StepId: Copy>(
    store: &impl MigrationStore<StepId>,
    plan: &Plan<StepId>,
    cancelled: impl Fn() -> bool,
) -> Result<Run, Error> {
    let mut session = store.begin_migration()?;
    match session.observe()? {
        GuardedObservation::Ready(stamp) if stamp == plan.observed => {}
        GuardedObservation::Ready(_) => return Err(Error::StalePlan),
        GuardedObservation::RecoveryRequired => return Err(Error::Dirty),
    }
    if session.id() != plan.catalog || session.binding() != plan.binding {
        return Err(Error::StaleCatalog);
    }
    let fresh = session.probe();
    for &(step, expected) in &plan.steps {
        let c = session.capabilities(step).ok_or(Error::MissingStep)?;
        if c != expected {
            return Err(Error::StaleCatalog);
        }
        check_backend(fresh, c)?;
        plan.policy.check(c)?;
    }

    let mut last = plan.observed.version;
    for &(step, _) in &plan.steps {
        if cancelled() {
            return Ok(Run::Stopped {
                last,
                error: Error::Cancelled,
            });
        }
        match session.apply(step) {
            StepOutcome::Committed => last = step.to,
            StepOutcome::NotCommitted(error) => {
                return Ok(Run::Stopped { last, error });
            }
            StepOutcome::RecoveryRequired(error) => {
                return Ok(Run::NeedsRecovery {
                    last_confirmed: last,
                    attempted: step.to,
                    error,
                });
            }
        }
    }
    debug_assert_eq!(last, plan.target);
    Ok(Run::Finished(last))
}
