use crate::{
    BackendSupport, Catalog, Error, Plan, Stamp, Step, Version, check_backend,
};

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

/// A planned step that [`execute`] has cleared to run.
///
/// Only the runner constructs one, so a session can be driven only through
/// [`execute`]: a direct `apply` cannot skip the guarded re-observation, the
/// stale-plan check or the full preflight. That closes the hole where a
/// caller holding a session overwrote an unresolved intent. The runner also
/// re-observes before every step after the first. Adapters still refuse a
/// pending intent inside `apply` as defence in depth, because an uncooperative
/// writer can appear between the runner's observation and the write.
#[derive(Debug)]
pub struct AuthorizedStep<S>(Step<S>);

impl<S: Copy> AuthorizedStep<S> {
    pub fn step(&self) -> Step<S> {
        self.0
    }
}

/// A store that can be migrated.
///
/// Adapter obligations, in addition to the method contracts:
///
/// - Observation (`observe` on a session, and any unguarded observation used
///   for planning) must return a coherent snapshot. A single read is enough
///   for a single atomically replaced document. A store spread over several
///   files must re-read until the parts agree, or reject the state; a stamp
///   must never pair parts that never coexisted. Do not take the guard to
///   observe, because observation must still report a live writer's intent.
///   Anything torn is rejected as stale or dirty by the guarded
///   re-observation.
/// - Recovery must validate evidence, not merely match it. An intent whose
///   old, target and rollback records are inconsistent with each other (for
///   example a rollback that changes the version, or revisions that are not
///   the expected successors) is `RecoveryBlocked`, never "recovered".
/// - Guard ownership stays inside the session, held from `begin_migration`
///   until the session is dropped.
pub trait MigrationStore<S> {
    type Session<'a>: MigrationSession<S>
    where
        Self: 'a;

    /// Acquires coordination. Performs no repair and no format write.
    fn begin_migration(&self) -> Result<Self::Session<'_>, Error>;
}

/// A guarded migration session.
///
/// Pre-write outcome mapping is an adapter obligation. Before the first
/// durable change of a step, report `NotCommitted` only if the old state is
/// still proven healthy: a readable, healthy store at another version is
/// `NotCommitted(UnexpectedVersion)`, an unsupported backend or a refused
/// meaningful field is `NotCommitted`, and an unreadable, inconsistent or
/// intent-bearing state fails closed as `RecoveryRequired`. Once a durable
/// intent may be visible, any failure is `RecoveryRequired`; never claim
/// `Committed` from a version string after an ambiguous write.
///
/// Every `apply` must re-check its own preconditions (version, intent,
/// fresh `probe`), because the runner does not re-validate store contents
/// between steps. It only re-observes the stamp's store, chain and version.
pub trait MigrationSession<S>: Catalog<S> {
    /// Coherent observation under the guard. A pending intent is
    /// `RecoveryRequired`; a foreign intent is `ChainMismatch`.
    fn observe(&self) -> Result<GuardedObservation, Error>;

    /// Fresh, read-only backend probe; never the cached `backend()`.
    fn probe(&self) -> BackendSupport;

    /// Runs one step. Only [`execute`] can call this.
    fn apply(&mut self, step: AuthorizedStep<S>) -> StepOutcome;
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

/// Runs `plan` under the adapter's coordination.
///
/// Re-observes under the guard and rejects a stale plan, a changed catalog
/// and any capability or backend change before the first write. Steps commit
/// one at a time; the result reports the committed prefix, not a chain
/// transaction. Cancellation is checked between steps.
pub fn execute<S: crate::StepId>(
    store: &impl MigrationStore<S>,
    plan: &Plan<S>,
    cancelled: impl Fn() -> bool,
) -> Result<Run, Error> {
    let mut session = store.begin_migration()?;
    match session.observe()? {
        GuardedObservation::Ready(stamp) if stamp == *plan.observed() => {}
        GuardedObservation::Ready(_) => return Err(Error::StalePlan),
        GuardedObservation::RecoveryRequired => return Err(Error::Dirty),
    }
    if session.id() != plan.catalog() || session.binding() != plan.binding() {
        return Err(Error::StaleCatalog);
    }
    let fresh = session.probe();
    for &(step, expected) in plan.steps() {
        let c = session.capabilities(step).ok_or(Error::MissingStep)?;
        if c != expected {
            return Err(Error::StaleCatalog);
        }
        check_backend(fresh, c)?;
        plan.policy().check(c)?;
    }

    let mut last = plan.observed().version;
    for (i, &(step, _)) in plan.steps().iter().enumerate() {
        if cancelled() {
            return Ok(Run::Stopped {
                last,
                error: Error::Cancelled,
            });
        }
        if i > 0
            && let Err(error) = reobserve(&session, plan.observed(), last)
        {
            return Ok(Run::Stopped { last, error });
        }
        match session.apply(AuthorizedStep(step)) {
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
    debug_assert_eq!(last, plan.target());
    Ok(Run::Finished(last))
}

/// Between steps the store must still be the planned one, healthy, at the
/// last confirmed version. Revisions advance per step and are adapter-defined,
/// so they are not compared.
fn reobserve<S>(
    session: &impl MigrationSession<S>,
    planned: &Stamp,
    last: Version,
) -> Result<(), Error> {
    match session.observe()? {
        GuardedObservation::Ready(s)
            if s.store == planned.store
                && s.chain == planned.chain
                && s.version == last =>
        {
            Ok(())
        }
        GuardedObservation::Ready(_) => Err(Error::StalePlan),
        GuardedObservation::RecoveryRequired => Err(Error::Dirty),
    }
}
