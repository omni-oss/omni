use crate::{
    BackendSupport, CatalogId, Chain, ChainBinding, Direction, Error, Policy,
    Stamp, StepCapabilities, StepId, Version,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step<S> {
    pub from: Version,
    pub to: Version,
    pub id: S,
}

pub type PlannedStep<S> = (Step<S>, StepCapabilities);

/// A fully validated migration path.
///
/// Fields are private and only [`build_plan`] constructs one, so nothing
/// external can assemble or edit a plan. Capability preflight describes what
/// can be attempted, not what arbitrary later data can represent.
#[derive(Debug)]
pub struct Plan<S> {
    observed: Stamp,
    catalog: CatalogId,
    binding: ChainBinding,
    direction: Direction,
    target: Version,
    policy: Policy,
    steps: Vec<PlannedStep<S>>,
}

impl<S> Plan<S> {
    pub fn observed(&self) -> &Stamp {
        &self.observed
    }

    pub fn catalog(&self) -> CatalogId {
        self.catalog
    }

    pub fn binding(&self) -> ChainBinding {
        self.binding
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    pub fn target(&self) -> Version {
        self.target
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    pub fn steps(&self) -> &[PlannedStep<S>] {
        &self.steps
    }
}

/// Read-only description of a chain's steps for one backend.
///
/// `CatalogId` binds the immutable chain and the dispatch/capability
/// definition for the selected backend; [`ChainBinding`] pins which chain that
/// is. Implementations must be pure: no I/O and no writes.
pub trait Catalog<S> {
    fn id(&self) -> CatalogId;

    fn binding(&self) -> ChainBinding;

    /// What this backend can honor, independent of any step. This may be
    /// cached when the catalog is built, so it is for planning only; guarded
    /// preflight and every step use a fresh
    /// [`probe`](crate::MigrationSession::probe).
    fn backend(&self) -> BackendSupport;

    /// Stable for this catalog and backend; `None` means the step is not
    /// implemented.
    fn capabilities(&self, step: Step<S>) -> Option<StepCapabilities>;
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

/// Plans every step before any write.
///
/// `observed` must come from an adapter observation; a chain name that
/// differs from the catalog's is `ChainMismatch`, and a chain whose digest
/// differs from the catalog's is `StaleCatalog`.
pub fn build_plan<S: StepId>(
    chain: &Chain<'_, S>,
    catalog: &impl Catalog<S>,
    observed: Stamp,
    target: Version,
    direction: Direction,
    policy: Policy,
) -> Result<Plan<S>, Error> {
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
            for e in &chain.edges()[a..b] {
                push(Step {
                    from: e.previous,
                    to: e.version,
                    id: e.upgrade,
                })?;
            }
        }
        Direction::Downgrade if b < a => {
            for e in chain.edges()[b..a].iter().rev() {
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
