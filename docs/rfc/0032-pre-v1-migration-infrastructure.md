# RFC 0032: Pre-v1 migration infrastructure

**Status:** Proposed. This PR is design only. The Rust below is illustrative, not compiled or shipped.
**Readers:** Omni maintainers reviewing the contract and contributors who will prototype it.

## Goal and scope

Prove, with two disposable fixtures, whether stores with different durability guarantees can share a migration planner and runner. The proposed leaf crate, `omni_migration`, knows about versions, ordered transitions, policy and outcomes. It does not know about Omni configuration, filesystems, cache keys, or source-store internals.

Before v1, do not migrate any production data in either direction and do not wire migrations into startup, runtime, CLI, or existing stores. This PR adds only planning documents. A green, independently reviewed RFC PR and maintainer approval gate the first implementation slice; the parent then starts that work with `swe-execute`. Even a successful fixture prototype does not authorize production adoption.

### Assessment

| Question                       | Assessment                                                                                                                                                                                                                          |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| How much changes?              | One docs PR now; later, a new internal contract, a leaf crate and two fixtures. No production callers, data, or persisted formats change.                                                                                           |
| How much is unknown?           | The main unknown is whether both adapters can report truthful commit outcomes and hold coordination through interruption without storage-specific branches in the runner. Test that first with both fixtures.                       |
| How hard is it to undo?        | Revert docs or remove the unwired prototype and disposable fixtures. A real migration is a different, potentially irreversible project.                                                                                             |
| Who and how long?              | One designer and an independent maintainer reviewer now. Start with one implementer and a one- to two-day timebox for the riskiest proof; report any unresolved assumptions before expanding the prototype or extracting the crate. |
| Kind, risk and execution shape | Medium / Low for the docs change itself; Large / High planning rigor for the proposed persistent-data and concurrency contract. Solo design, independent review, then contract-first prototype; no nested agents.                   |
| Checks and next step           | Check paths, formatting and PR scope now. After approval, exercise both adapters end to end, then run scoped checks per slice and broad checks once at integration.                                                                 |
| What would increase scope?     | Any real store, caller, public API, cross-platform durability promise beyond the tested fixture backend, or release rollout requires reassessment and a separate approval.                                                          |

## Existing version domains and independent decisions

These domains have distinct owners and reasons to change. None is an Omni release number, and a change in one does not imply a bump in another.

| Domain                                  | Observed in `main`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Semantic encoding                       | [`EffectiveTaskConfig::encode` and `VERSION`](../../crates/omni_collector/src/effective_task.rs) frame the bytes with `effective-task-config/v1`. The digest is not yet wired into task keys.                                                                                                                                                                                                                                                                                                                                                                   | Version the encoding when its meaning changes; do not use that tag as the whole task-key version.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Whole task key                          | [`Collector::get_digest` and `collect`](../../crates/omni_collector/src/collector.rs) combine invocation, dependency and file inputs and use the resulting digest for the output directory. See also [`cache_manager.rs`](../../crates/omni_task_executor/src/cache_manager.rs), [`ProjectHasher`](../../crates/omni_context/src/project_hasher.rs) and [`WorkspaceHasher::hash`](../../crates/omni_context/src/workspace_hasher.rs). The latter currently unions file inputs into synthetic project tasks; that is not the finalized per-task semantic digest. | [F06.3](0024-tiered-project-discovery-plugins/implementation/features/F06.2-canonical-digest.md#for-f063) versions the whole key independently and includes the semantic digest even with `cache.key.defaults: false`. The workspace hash includes the digest via deterministic per-task contributions, not just the synthetic file-input union. Platform contribution is opt-in via user configuration, not an implicit OS salt. Take a release-noted cold start: no fallback lookup, promotion, rewrite, or population of new keys from legacy entries. Test executor, dry-run, project hash and lookup parity, workspace-hash sensitivity, and opt-in/default behavior in F06.3. Do not add a migration/version check in the per-task hash loop. |
| Cache storage layout and entry encoding | [`Collector::collect`](../../crates/omni_collector/src/collector.rs) derives `<cache>/<encoded-project>/output/<encoded-digest>`; [`hybrid.rs`](../../crates/omni_cache/src/cache/impls/hybrid.rs) publishes and reads entry metadata, [`CachedOutput` and `PATH_FORMAT_*`](../../crates/omni_cache/src/cache/cached_output.rs) describe stored paths, and [`partition.rs`](../../crates/omni_cache/src/cache/impls/partition.rs) owns trust partitioning and its epoch. These are not one global migration marker.                                             | A new key selects new entries, not new storage encoding or trust rules. F06.3 does not change the cache storage layout or migrate entries.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Remote-source layout                    | [`LAYOUT_VERSION`, `layout`, `validate_layout_marker` and `initialize_layout`](../../crates/omni_remote_source/src/manager/default_impl.rs) use store-root `VERSION` (`1`); new values above the supported one are refused, and create mode initializes a missing marker. `.layout.lock` serializes marker initialization and recheck only; existing-marker access does not take it. The source [lockfile](../../crates/omni_lockfile/src/lockfile_data.rs) has its own document tag, not the store marker.                                                     | Resolution of [F08 D1](0024-tiered-project-discovery-plugins/implementation/features/F08.1-lease-gc-reference-protocol.md#open-decisions): leave store `VERSION` unchanged, superseding the earlier conditional bump recommendation. Older binaries need not honor leases or any future migration guard; this decision does not make mixed-version prune safe. Do not rely on `.layout.lock` to make older binaries participate. A future live migration needs a separately approved exclusion or offline strategy.                                                                                                                                                                                                                                 |

Neither F06.3 nor F08 D1 depends on this crate. This PR records their boundary decisions; it neither implements F06.3 nor enables a migration.

## Contract

- `Version(u32)` is an internal, opaque identifier. An adapter owns stable mappings from on-disk tokens, with separate missing, malformed and well-formed-but-unknown errors (and separate I/O errors). Do not interpret absence as a version, increment integers, infer format from app release, or sort versions numerically. Initialization, if supported, is separate and explicit.
- A chain is a base version plus an immutable, validated slice of transitions in predecessor order. Each transition has `previous`, `version`, a typed upgrade `StepId` and an optional **separately implemented** downgrade `StepId`. Validate continuity, unique versions (including the base), and unique step IDs, including across directions. Use indices for order and a registry closed over the adapter's typed IDs. No graph, linked heap nodes or dynamic plugins.
- A store records the **chain identity** it belongs to: a stable, adapter-owned `ChainName` (for example `"pins"`) written in the authoritative state beside the version token. The version token is meaningful only within that chain. The adapter verifies the name during observation, before it can return `Ready(version)`; a missing or malformed name is an observation error like a missing or malformed version, and a well-formed name that differs from the catalog's is `ChainMismatch`. `ChainMismatch` is reserved for this store-name mismatch and is never retried, repaired, planned around or treated as unknown-newer; it refuses ordinary access and explicit planning with zero writes. Because verification happens at observation, the up-to-date path stays one comparison. The catalog additionally exposes a `ChainBinding` (name plus a digest of the validated base, ordered `(previous, version)` pairs and step IDs). The plan binds it, so a plan or catalog built for a different or edited chain (a binding or digest mismatch, at planning and at guarded preflight alike) is `StaleCatalog`, never `ChainMismatch`. The store records only the name, not the digest, so appending a transition keeps existing stores readable.
- Ordinary access does not migrate. A caller refuses an incompatible newer format, whether recognized as newer by chain order or unrecognized by its adapter. An explicit upgrade must select a higher chain position; an explicit downgrade must select a lower one and have the entire reverse path before any format writes. An equal-version explicit request is `WrongDirection`, not `UpToDate`. Neither planning nor execution treats an upgrade as having an implicit inverse.
- A caller observes state. It only supplies `Ready(version)` when the adapter has established health, not merely read a version marker when partial state is possible. Observation errors are not healthy states. The caller must obtain a coherent snapshot (for example, use its existing read guard if separate files could change between reads). Once health has been observed, current-version access is one version comparison, returning `UpToDate` without touching the registry, opening a session, allocating, or adding I/O or logging. It is not a lock protecting subsequent ordinary reads; callers still own the consistency of those reads.
- Planning is deterministic and read-only. Bind the plan to the observed store identity, revision and version, the immutable registry/catalog identity, direction, target and policy. A revision must detect relevant same-version edits, including replacement of a store; define it over the complete authoritative state or use a non-reused generation updated by every cooperating writer. An adapter unable to supply that evidence cannot use the shared runner as sketched. Plans are opaque, in-process values, not user input or replayable serialized jobs.
- Capabilities belong to each direction's implementation. `atomic` means publication of **that step's logical format/data**, not the entire chain, is old-complete or new-complete on the supported backend. `lossless` means successful conversion preserves all meaningful data; refusal is allowed. `derivable` matters only when lossy: every discarded value can be reconstructed deterministically from retained authoritative inputs (including the needed derivation definition). It does not imply a downgrade implementation exists.
- Capabilities are checked **before a step runs**, against both policy and the backend. Lossless/derivable are properties of the step's code and are declared in the catalog; the backend reports what it can honor (`BackendSupport`) as two separate facts. `durable` (exclusive same-directory staging, file sync, directory sync and atomic replacement of single small files such as intent and marker documents; everything either fixture's commit and recovery protocol needs, atomic or not) is required by **every** step; a backend without it refuses all steps with `UnsupportedBackend`, regardless of `allow_non_atomic`. `atomic_publish` (atomic publication of a step's whole payload, so all data of a step becomes visible together; never small-file replacement) is additionally required by steps declaring `atomic`. A step declaring `atomic` on a backend without `atomic_publish` is **refused** with `UnsupportedBackend`: no degradation to a non-atomic protocol, and `allow_non_atomic` does not override it, because that opt-in covers steps designed as non-atomic, not a weakened atomic one. The check runs at planning and again at guarded preflight for every step, before any format write; it is an error, not `NotCommitted`. Each step's `apply` takes a fresh read-only probe (`probe()`) before staging any write and checks that result, so a mid-chain flip is observable; and a step that discovers the loss mid-chain before mutating returns `NotCommitted(UnsupportedBackend)`. If support is lost after a step began mutating, the outcome is `RecoveryRequired` under the general rule, never `NotCommitted`. The cached probe taken when the session opens is used only for planning and guarded preflight; no probe may write.
- Default policy rejects lossy steps, and this sketch also defaults to requiring atomic steps. Derivable loss, irreversible loss and permission to use a non-atomic step are independent opt-ins. No opt-in overrides missing implementation, stale state, unsupported durability (`durable`), unknown versions or recovery. The non-atomic opt-in shape is a fixture-level proposal, not a CLI or release policy.
- The session owns the guard from re-observation through the last step and cleanup. `begin_migration` may acquire or create coordination resources, but may not change the format, data or recovery state. Re-observe under the guard; reject stale state without replanning, even if a peer already reached the target. Recheck all planned IDs, capabilities and policy before any format writes. A data-dependent check on a later step may require the predecessor's actual output. An adapter performs that check on the guarded input before the step mutates it; failure stops at the last committed version, without an automatic inverse.
- The adapter owns per-step publication, durable commit, recovery evidence and rollback. `Committed` means the next version and its data are durably published. `NotCommitted(error)` means the old version is proven live and healthy, including any local rollback; inert temp files may remain. `RecoveryRequired(error)` means the adapter cannot establish a healthy old or new state; do not infer success from a plausible marker or retry the chain. The runner reports the last **confirmed** version separately from an unresolved step. Recovery is a separate guarded adapter operation. Rollback restores an interrupted or failed operation's prior state; downgrade converts a healthy committed format to an earlier one by an explicit step and policy.

## Core Rust reference sketch

Helper types such as `Error`, `StoreId`, `Revision`, `CatalogId`, `ChainName`, `ChainBinding`, `digest_chain`, diagnostic plumbing and derives are abbreviated. `ChainName` is `Copy` (it is passed by value, including where `build_plan` reads `observed.chain` before moving `observed`). A `Stamp` is captured as part of the same coherent observation as `Ready`; building or moving it belongs to the caller or the cold path. `gate_access` is only for ordinary access and takes a lazy compatibility check for mismatched versions. It does not construct a migration plan. Explicit planning requires a healthy observation and calls `build_plan` directly, including when the observed version equals the requested target.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Version(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StoreState {
    Ready(Version),
    RecoveryRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    store: StoreId,
    chain: ChainName,
    revision: Revision,
    version: Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StepCapabilities {
    atomic: bool,
    lossless: bool,
    derivable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BackendSupport {
    durable: bool,        // Required by every step.
    atomic_publish: bool, // Required only by `atomic` steps.
}

#[derive(Clone, Copy, Default)]
struct Policy {
    allow_non_atomic: bool,
    allow_derivable_loss: bool,
    allow_irreversible_loss: bool,
}

impl Policy {
    fn check(self, c: StepCapabilities) -> Result<(), Error> {
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

#[derive(Clone, Copy)]
enum Direction {
    Upgrade,
    Downgrade, // Caller must explicitly request it.
}

struct Transition<StepId> {
    previous: Version,
    version: Version,
    upgrade: StepId,
    downgrade: Option<StepId>,
}

struct Chain<'a, StepId> {
    base: Version,
    edges: &'a [Transition<StepId>], // Construct only via validation.
}

impl<'a, StepId: Copy + Eq + std::hash::Hash> Chain<'a, StepId> {
    fn new(base: Version, edges: &'a [Transition<StepId>]) -> Result<Self, Error> {
        let mut versions = std::collections::HashSet::from([base]);
        let mut ids = std::collections::HashSet::new();
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

    // Digest of base, ordered (previous, version) pairs and step IDs.
    fn binding(&self, name: ChainName) -> ChainBinding {
        ChainBinding { name, digest: digest_chain(self.base, self.edges) }
    }

    fn index(&self, v: Version) -> Result<usize, Error> {
        if v == self.base {
            return Ok(0);
        }
        self.edges.iter().position(|e| e.version == v)
            .map(|i| i + 1)
            .ok_or(Error::UnknownVersion(v))
    }
}

#[derive(Clone, Copy)]
struct Step<StepId> {
    from: Version,
    to: Version,
    id: StepId,
}

struct Plan<StepId> {
    observed: Stamp,
    catalog: CatalogId,
    binding: ChainBinding,
    direction: Direction,
    target: Version,
    policy: Policy,
    steps: Vec<(Step<StepId>, StepCapabilities)>,
}

trait Catalog<StepId> {
    fn id(&self) -> CatalogId;
    fn binding(&self) -> ChainBinding;
    // Read-only; what this backend can honor, independent of any step.
    fn backend(&self) -> BackendSupport;
    // Read-only and stable for this catalog/backend; None means not implemented.
    fn capabilities(&self, step: Step<StepId>) -> Option<StepCapabilities>;
}

enum Check<T> {
    UpToDate,
    Work(T),
}

fn gate_access<T>(
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

fn check_backend(b: BackendSupport, c: StepCapabilities) -> Result<(), Error> {
    if !b.durable {
        return Err(Error::UnsupportedBackend); // Every step; no opt-in overrides.
    }
    if c.atomic && !b.atomic_publish {
        return Err(Error::UnsupportedBackend); // Refuse; never degrade.
    }
    Ok(())
}

fn build_plan<StepId: Copy + Eq + std::hash::Hash>(
    chain: &Chain<'_, StepId>,
    catalog: &impl Catalog<StepId>,
    observed: Stamp,
    target: Version,
    direction: Direction,
    policy: Policy,
) -> Result<Plan<StepId>, Error> {
    // Name check is defence in depth: observation already verified it.
    if observed.chain != catalog.binding().name {
        return Err(Error::ChainMismatch);
    }
    // Edited or different chain: binding/digest mismatch, not a store mismatch.
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
                push(Step { from: e.previous, to: e.version, id: e.upgrade })?;
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
        _ => return Err(Error::WrongDirection), // Includes equal versions.
    }
    Ok(Plan { observed, catalog: catalog.id(), binding: catalog.binding(), direction, target, policy, steps })
}

enum StepOutcome {
    Committed,
    NotCommitted(Error),
    RecoveryRequired(Error),
}

enum GuardedObservation {
    Ready(Stamp),
    RecoveryRequired,
}

trait MigrationStore<StepId> {
    type Session<'a>: MigrationSession<StepId> where Self: 'a;
    fn begin_migration(&self) -> Result<Self::Session<'_>, Error>;
}

trait MigrationSession<StepId>: Catalog<StepId> {
    fn observe(&self) -> Result<GuardedObservation, Error>;
    fn apply(&mut self, step: Step<StepId>) -> StepOutcome;
}

enum Run {
    Finished(Version),
    Stopped { last: Version, error: Error },
    NeedsRecovery { last_confirmed: Version, attempted: Version, error: Error },
}

fn execute<StepId: Copy>(
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
    for &(step, expected) in &plan.steps {
        let c = session.capabilities(step).ok_or(Error::MissingStep)?;
        if c != expected {
            return Err(Error::StaleCatalog);
        }
        check_backend(session.backend(), c)?;
        plan.policy.check(c)?;
    }

    let mut last = plan.observed.version;
    for &(step, _) in &plan.steps {
        if cancelled() {
            return Ok(Run::Stopped { last, error: Error::Cancelled });
        }
        match session.apply(step) {
            StepOutcome::Committed => last = step.to,
            StepOutcome::NotCommitted(error) => return Ok(Run::Stopped { last, error }),
            StepOutcome::RecoveryRequired(error) => {
                return Ok(Run::NeedsRecovery { last_confirmed: last, attempted: step.to, error });
            }
        }
    }
    debug_assert_eq!(last, plan.target);
    Ok(Run::Finished(last))
}
```

`CatalogId` binds the immutable chain and the dispatch/capability definition for the selected backend; `ChainBinding` pins which chain that is. A chain whose digest differs from the catalog's (`StaleCatalog`), or a store whose recorded name differs (`ChainMismatch`), can never produce a plan. Nothing external can assemble or edit a `Plan`; only the validated chain and planner make one. Capability preflight describes what can be attempted, not what arbitrary later data can represent.

Use synchronous fixture sessions first. If an async harness is needed, move the **whole** execution into a blocking worker that acquires and owns the session; never let cancellation of an awaiting task release a guard while a detached writer is still running. Cancellation is cooperative between steps. Dropping an awaiter is not a rollback or an outcome; a successor must observe under coordination after the worker settles. A panic or process death must be safe by protocol, not by running a destructor. The core adds no async runtime dependency.

## Reference fixtures

Both fixtures are invented and run only against temporary directories. The shared runner gets the same interface in both cases; the adapters do not share commit code just to look alike.

### `LockfileStore`: atomic, possibly refusing downgrade

Use one authoritative JSON document containing chain name (`"pins"`), version, generation and all data. Example versions are `"pins-v1"` → `Version(7)` and `"pins-v2"` → `Version(42)`. A well-formed tag is `pins-v` followed by one or more ASCII digits; only listed strings are supported. A fresh token or `incarnation` is supplied only at explicit fixture creation; every authoritative writer bumps the document generation without wrap. One non-replaced `.migration.guard` coordinates observation, writes and recovery (all writers must participate, or exclude them offline). Do not lock the inode being replaced or unlink the guard. The real [`Lockfile::save_inner`](../../crates/omni_lockfile/src/lockfile.rs) is an example of single-file publication, **not** this recovery protocol: [`atomic_file::write`](../../crates/omni_utils/src/atomic_file.rs) explicitly does not sync the parent directory.

```rust
const PINS_V1: Version = Version(7);
const PINS_V2: Version = Version(42);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PinsStep { AddNotes, BackToPlainPins }

const PINS_EDGES: &[Transition<PinsStep>] = &[Transition {
    previous: PINS_V1,
    version: PINS_V2,
    upgrade: PinsStep::AddNotes,
    downgrade: Some(PinsStep::BackToPlainPins),
}];

fn pins_version(tag: Option<&serde_json::Value>) -> Result<Version, Error> {
    match tag {
        None => Err(Error::MissingVersion),
        Some(serde_json::Value::String(s)) => match s.as_str() {
            "pins-v1" => Ok(PINS_V1),
            "pins-v2" => Ok(PINS_V2),
            _ if well_formed_pin_tag(s) => Err(Error::UnknownTag),
            _ => Err(Error::MalformedVersion),
        },
        Some(_) => Err(Error::MalformedVersion),
    }
}

struct LockfileStore { root: PathBuf, fs: DurableFixtureFs }
struct PinsSession<'a> {
    store: &'a LockfileStore,
    catalog: PinsCatalog, // backend probed once in begin_migration
    _guard: ExclusiveGuard,
}

impl MigrationStore<PinsStep> for LockfileStore {
    type Session<'a> = PinsSession<'a>;
    fn begin_migration(&self) -> Result<PinsSession<'_>, Error> {
        let guard = self.fs.lock_exclusive(self.root.join(".migration.guard"))?;
        let catalog = PinsCatalog { backend: self.fs.probe(), binding: pins_binding()? };
        Ok(PinsSession { store: self, catalog, _guard: guard })
    }
}

// A pure descriptor: planning can use PinsCatalog without opening a session.
// `backend` is probed once, read-only, when the store or session is opened (or
// taken from fixture configuration) and cached for planning and preflight only;
// `apply` takes a fresh probe(). No probe writes.
struct PinsCatalog { backend: BackendSupport, binding: ChainBinding }

// The binding is computed from the validated chain, never hand-written, so it
// cannot drift from PINS_EDGES.
fn pins_binding() -> Result<ChainBinding, Error> {
    Ok(Chain::new(PINS_V1, PINS_EDGES)?.binding(ChainName::PINS))
}

impl Catalog<PinsStep> for PinsCatalog {
    fn id(&self) -> CatalogId { PINS_CATALOG }
    fn binding(&self) -> ChainBinding { self.binding } // from pins_binding(), name "pins"
    fn backend(&self) -> BackendSupport { self.backend } // cached at open; planning and preflight only
    fn capabilities(&self, s: Step<PinsStep>) -> Option<StepCapabilities> {
        match (s.from, s.to, s.id) {
            (PINS_V1, PINS_V2, PinsStep::AddNotes)
            | (PINS_V2, PINS_V1, PinsStep::BackToPlainPins) => {
                Some(StepCapabilities { atomic: true, lossless: true, derivable: false })
            }
            _ => None,
        }
    }
}

impl Catalog<PinsStep> for PinsSession<'_> {
    fn id(&self) -> CatalogId { PINS_CATALOG }
    fn binding(&self) -> ChainBinding { self.catalog.binding() }
    fn backend(&self) -> BackendSupport { self.catalog.backend() } // cached; planning and preflight only
    fn capabilities(&self, s: Step<PinsStep>) -> Option<StepCapabilities> {
        self.catalog.capabilities(s) // pure table; no backend I/O
    }
}

impl MigrationSession<PinsStep> for PinsSession<'_> {
    fn observe(&self) -> Result<GuardedObservation, Error> {
        // A pending intent records its ChainName, so recovery stays reachable
        // even when the document's chain name is unreadable. A foreign store's
        // intent is ChainMismatch, not this chain's RecoveryRequired.
        if let Some(name) = self.store.pending_intent_chain()? {
            if name != ChainName::PINS {
                return Err(Error::ChainMismatch);
            }
            return Ok(GuardedObservation::RecoveryRequired);
        }
        self.store.check_chain_name()?;
        // Read and validate the whole document, not just its version string.
        // The chain-name check inside read_document is defence in depth;
        // check_chain_name() already ran.
        Ok(GuardedObservation::Ready(self.store.read_document()?.stamp()))
    }

    fn apply(&mut self, s: Step<PinsStep>) -> StepOutcome {
        // Fresh read-only probe before any write, staging included; the cached
        // session value cannot see a mid-chain flip.
        let Some(c) = self.capabilities(s) else {
            return StepOutcome::NotCommitted(Error::MissingStep);
        };
        if let Err(e) = check_backend(self.store.fs.probe(), c) {
            return StepOutcome::NotCommitted(e);
        }
        let old = match self.store.read_document() {
            Ok(doc) if doc.version() == s.from => doc,
            Ok(_) => return StepOutcome::RecoveryRequired(Error::UnexpectedVersion),
            Err(e) => return StepOutcome::RecoveryRequired(e),
        };
        let next = match (s.id, &old.body) {
            (PinsStep::AddNotes, Body::V1 { pins }) => Body::V2 {
                pins: pins.clone(),
                notes: Notes::default(),
            },
            (PinsStep::BackToPlainPins, Body::V2 { pins, notes }) if notes.is_empty() => {
                Body::V1 { pins: pins.clone() }
            }
            (PinsStep::BackToPlainPins, Body::V2 { .. }) => {
                return StepOutcome::NotCommitted(Error::MeaningfulNotes);
            }
            _ => return StepOutcome::RecoveryRequired(Error::UnexpectedVersion),
        };
        let replacement = match old.rewrite_checked(s.to, next) {
            Ok(doc) => doc,
            Err(e) => return StepOutcome::NotCommitted(e),
        };
        self.publish(&old, replacement)
    }
}

impl PinsSession<'_> {
    fn publish(&self, old: &Document, next: Document) -> StepOutcome {
        let fs = &self.store.fs;
        let stage = match fs.stage_document_and_sync(&next) {
            Ok(stage) => stage,
            Err(e) => return StepOutcome::NotCommitted(e),
        };
        let intent = Intent::new(ChainName::PINS, old.fingerprint(), next.fingerprint());
        // Each publish uses a same-directory exclusive temp + file sync +
        // replacement rename + parent directory sync; differentiate failures.
        match fs.publish_intent_durably(&intent) {
            Ok(()) => {}
            Err(PublishError::BeforeRename(e)) => return StepOutcome::NotCommitted(e),
            Err(PublishError::Uncertain(e)) => return StepOutcome::RecoveryRequired(e),
        }
        // Keep the durable intent if replacement or directory sync fails.
        if let Err(e) = fs.replace_document_and_sync_parent(stage) {
            return StepOutcome::RecoveryRequired(e);
        }
        // Data commit is now certain. If unlink fails and intent remains, block
        // and recover. If unlink is visible but its sync fails, either it stays
        // absent or the next boot sees an intent for already durable data.
        match fs.clear_intent_after_durable_document() {
            Ok(()) => StepOutcome::Committed,
            Err(e) => StepOutcome::RecoveryRequired(e),
        }
    }
}
```

Here `DurableFixtureFs` is a contract for one explicitly supported, local fixture backend: exclusive same-directory staging, write/flush/file sync, atomic replacement and directory sync (including initial root creation before use). Refuse unsupported backends in preflight: `probe()` reports `durable: false` when exclusive staging, file sync, directory sync or atomic replacement of single small files (intent and marker documents) cannot be confirmed, which refuses every step of both fixtures, and `atomic_publish: false` when atomic publication of a step's whole payload cannot be confirmed, which refuses only the lockfile's atomic steps. Small-file atomic intent and marker publication stays under `durable`; the index fixture runs on `durable: true, atomic_publish: false` with its non-atomic steps allowed. `clear_intent_after_durable_document` reports a failure when removal cannot be established, including an ambiguous unlink; if only the removal's directory sync fails after successful unlink, it may still report success for the **already durable document**, since a resurrected intent can safely block and be resolved on restart. Do not use that allowance before data is durable.

Every writer, including a same-version fixture editor, must refuse pending intents and durably publish its own fresh intent before touching authoritative state. This ensures an earlier intent whose removal was not synced cannot reappear alongside a later write. Keep the guard through this sequence.

The intent records the `ChainName` and the exact identities of the old and replacement documents (generation and content fingerprint). An intent whose chain name is missing or unreadable fails closed; it is never treated as no intent. Guarded recovery checks the live, fully validated document against the intent: if it matches either identity, sync it and its parent before resolving and clearing intent; report which was recovered, do not run a downgrade or publish the staged target. If neither matches, or durability cannot be re-established, refuse and retain recovery evidence. Before any document replacement, an absent intent is safe because either publication did not begin or the old document is intact. Missing document or tag with no intent is a missing observation error; invalid JSON or wrong/invalid tag shape is malformed; a syntactically valid unsupported tag is unknown. Strictly decode version-specific payloads; never silently drop unknown fields to make a downgrade pass.

### `IndexStore`: non-atomic, derivable data

Keep authoritative records and a fixed derivation definition independent of `VERSION` and the index. The marker tag `"indexed"` maps to `Version(90)` and has individually stored sorted-index shards; `"records-only"` maps to `Version(12)` and omits them. Dropping shards is lossy but derivable: shard selection, ordering and contents are fixed by the retained records and spec, with no auxiliary history. Rebuild by a separate downgrade step. Both directions are deliberately non-atomic on disk; a durable intent makes an interrupted step unavailable rather than pretending it is atomic. `VERSION` alone can still say `"indexed"` after shards have been removed.

```rust
const WITH_INDEX: Version = Version(90);
const WITHOUT_INDEX: Version = Version(12);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum IndexStep { DropShards, RebuildShards }

const INDEX_EDGES: &[Transition<IndexStep>] = &[Transition {
    previous: WITH_INDEX,
    version: WITHOUT_INDEX,
    upgrade: IndexStep::DropShards,
    downgrade: Some(IndexStep::RebuildShards),
}];

// begin_migration acquires the fixture's stable guard exactly as PinsSession
// does, but calls no repair, directory creation (other than guard resources),
// or format writes. The read-only planning catalog delegates to this same
// pure dispatch table. CatalogId binds the chain and derivation/backend.
impl Catalog<IndexStep> for IndexSession<'_> {
    fn id(&self) -> CatalogId { INDEX_CATALOG }
    // Computed from the validated chain at session open:
    // Chain::new(WITH_INDEX, INDEX_EDGES)?.binding(ChainName::INDEX).
    fn binding(&self) -> ChainBinding { self.binding }
    fn backend(&self) -> BackendSupport { self.backend } // probed once at open, read-only; planning and preflight only
    fn capabilities(&self, s: Step<IndexStep>) -> Option<StepCapabilities> {
        match (s.from, s.to, s.id) {
            (WITH_INDEX, WITHOUT_INDEX, IndexStep::DropShards) => {
                Some(StepCapabilities { atomic: false, lossless: false, derivable: true })
            }
            (WITHOUT_INDEX, WITH_INDEX, IndexStep::RebuildShards) => {
                Some(StepCapabilities { atomic: false, lossless: true, derivable: false })
            }
            _ => None,
        }
    }
}

impl MigrationSession<IndexStep> for IndexSession<'_> {
    fn observe(&self) -> Result<GuardedObservation, Error> {
        // Same order as PinsSession: the intent's recorded ChainName decides first.
        if let Some(name) = self.io.pending_intent_chain()? {
            if name != ChainName::INDEX {
                return Err(Error::ChainMismatch);
            }
            return Ok(GuardedObservation::RecoveryRequired);
        }
        self.io.check_chain_name()?;
        // Validate layout, authority, and its relation to the shards. A stamp
        // includes the non-reused root identity and all relevant revisions.
        self.io.observe_clean_layout()
    }

    fn apply(&mut self, s: Step<IndexStep>) -> StepOutcome {
        // Fresh probe before any write: `durable` is required (it covers the
        // atomic intent and marker documents); these steps are non-atomic so
        // `atomic_publish` is not. Loss here is NotCommitted.
        let Some(c) = self.capabilities(s) else {
            return StepOutcome::NotCommitted(Error::MissingStep);
        };
        if let Err(e) = check_backend(self.io.probe(), c) {
            return StepOutcome::NotCommitted(e);
        }
        // Load records and verify preconditions without mutations.
        let inputs = match self.io.read_verified_inputs(s.from) {
            Ok(inputs) => inputs,
            Err(e) => return self.io.classify_prewrite_failure(e),
        };
        // Reserve distinct target and rollback marker revisions in the intent.
        let intent = match self.io.begin_intent_durably(s.from, s.to, &inputs.revision) {
            Ok(intent) => intent,
            Err(PublishError::BeforeRename(e)) => return StepOutcome::NotCommitted(e),
            Err(PublishError::Uncertain(e)) => return StepOutcome::RecoveryRequired(e),
        };
        let update = match s.id {
            IndexStep::DropShards => self.io.remove_each_shard(),
            IndexStep::RebuildShards => self.io.write_each_shard(derive(&inputs)),
        };
        let commit = update
            .and_then(|_| self.io.verify_target_shards(s.to, &inputs))
            .and_then(|_| self.io.sync_changed_files_and_directories())
            .and_then(|_| self.io.publish_marker_durably(&intent.target_marker))
            .and_then(|_| self.io.clear_intent_after_durable_data());
        match commit {
            Ok(()) => StepOutcome::Committed,
            Err(e) => StepOutcome::RecoveryRequired(e),
        }
    }
}
```

Prewrite failure is `NotCommitted` only if the old layout is still known healthy; an inconsistency or unreadable state fails closed. The durable intent pins authoritative identity, records the `ChainName` (so `observe()` can report `RecoveryRequired` even when the document name is unreadable) and the exact old, target and rollback markers. Chain name, version and revision are published together as a small atomic marker document, **after** the shards are verified and synced; that does not make the shard changes atomic.

Recovery validates the authoritative identity and reads the marker under the guard. If it matches the intent's target marker, verify the target shards and sync the marker and directory before clearing intent and confirming the target. Never roll back a target that may already have been acknowledged. If it matches the old marker, restore the **before** layout by rebuilding shards or removing partial ones, verify and sync, and publish the reserved rollback marker (old version, fresh revision). If it already matches the rollback marker, verify and sync the restored layout before clearing intent. Anything else, including a mismatched shard set at a purportedly published target, stays blocked. This is rollback of an unresolved operation, not the plan's reverse step. Use the same cleanup rule as the lockfile fixture once the selected data and marker are durable. A resurrected intent can be resolved without undoing a confirmed commit.

## Worked acceptance cases

Port this table to fixtures after approval. Count format/data writes separately from guard acquisition and observation. Exercise the real fixture protocol with fault injection, not just a stub that returns the desired enum.

| Case                                 | Given / action                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Expected evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Lockfile round trip and refusal      | One v1 pin; upgrade, then explicitly downgrade first with empty notes, then in a separate run with a meaningful v2 note.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | First round trip preserves the pin and logical data; revisions advance. Second returns `NotCommitted(MeaningfulNotes)` with v2 bytes unchanged and no intent. Ordinary v1 access to v2 fails without any mutation.                                                                                                                                                                                                                                                                                                                              |
| Chain identity binding               | Open a store recorded as chain `"index"` with the pins catalog (and the reverse); a store with the chain name missing or malformed; a store whose version token is valid in both chains. Separately build a plan, then present a catalog whose chain was edited (changed step ID or reordered versions) at guarded preflight.                                                                                                                                                                                                                                                                                                                                                        | Observation fails with `ChainMismatch` (or missing/malformed) before `Ready`; ordinary access and planning refuse, zero writes, no recovery attempted. Edited chain is `StaleCatalog` at planning and at preflight (`ChainMismatch` only for the store's name). Appending a transition leaves the recorded name valid and the old store readable.                                                                                                                                                                                               |
| Intent chain name                    | Leave a pending intent and make the document's chain name unreadable or foreign, for each fixture.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Own-chain intent with unreadable name: `RecoveryRequired`; foreign intent name: `ChainMismatch`; zero writes.                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Backend capability gate              | Run the lockfile fixture on a fault backend reporting `durable: true, atomic_publish: false`, with and without `allow_non_atomic`; run the index fixture on the same backend. Separately report `durable: false` and run both fixtures with and without `allow_non_atomic`. Flip the fault backend's live support value to false after planning and before guarded preflight (set before `begin_migration` takes its cached probe, so the flip must happen between `begin_migration` and preflight by flipping the live value), and inside a later step before it mutates (visible only to that step's fresh `probe()`), using the three-version fixture from the prefix-commit row. | Lockfile upgrade and downgrade fail with `UnsupportedBackend` in both policies, no format write, no intent. Index plans and runs unchanged on the durable, non-atomic backend. With `durable: false`, every step of both fixtures is refused with `UnsupportedBackend` regardless of `allow_non_atomic`, with zero writes. Preflight flip is an error with zero writes; the in-step flip before the later step mutates is `Stopped { last: middle, error: UnsupportedBackend }` (from `NotCommitted(UnsupportedBackend)`), middle version live. |
| Full plan before writes              | Use a three-version chain with nonconsecutive, nonmonotonic IDs. Remove the downgrade on the **second** reverse leg; separately leave it declared but omit the implementation in the catalog.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | `NoDowngradePath` versus `MissingStep`; no first reverse write. Exercise duplicate versions/IDs, a disconnected predecessor, unknown source/target and distinct missing/malformed/unknown observations. Compare repeated plans for stable step order and capabilities.                                                                                                                                                                                                                                                                          |
| Policy is directional                | Plan `90 → 12`, then separately plan `12 → 90`. Stub a third step marked irreversible for policy checks, including on the second leg of a chain.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | Forward needs both non-atomic and derivable-loss opt-ins; reverse only needs non-atomic permission. Neither kind of loss permission grants the other. A failure anywhere in preflight leaves all steps no-write.                                                                                                                                                                                                                                                                                                                                |
| Guarded stale check                  | Plan from v1, then let a peer edit v1 (including an edit restored to the same logical data but with a new generation), replace the store, or reach v2 before execution. Separately change a capability at guarded preflight.                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Stale plan/catalog, no replanning even if target is now current; zero format writes.                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Lockfile interruption                | Inject failure before intent publication and at document replace, parent sync and intent cleanup. Kill a helper process after publishing intent and after replacing the document.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | Before visible intent, the old document is healthy and unchanged. Uncertain publication never claims commit from the version string. Reopen under the guard; recover a validated old or new document, or stay blocked for mismatched evidence.                                                                                                                                                                                                                                                                                                  |
| Non-atomic interruption              | Kill after the first shard is removed with the old marker still present, and again during a rollback run. Separately simulate an intent reappearing after a confirmed commit.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Observation is dirty despite matching `VERSION`. Ordinary access and runner refuse; guarded recovery can be retried and restores the old healthy layout byte-for-byte for the derivable shards. A normal successful cycle reconstructs the same shards from retained authority. A resurrected intent for a confirmed commit is finalized at the target, not rolled back.                                                                                                                                                                        |
| Prefix commit, not chain transaction | In a three-version lockfile fixture, make the first reverse step succeed and the second reject a meaningful field in its guarded input.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | `Stopped` at the middle version; the first step remains committed. Uncertain second publication instead reports `NeedsRecovery` with the middle version only as last confirmed.                                                                                                                                                                                                                                                                                                                                                                 |
| Cancellation and concurrency         | Barrier-block the first worker after intent, drop its awaiter, and race a second process for the same guard. Separately request cooperative cancellation at a step boundary.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | The second process cannot mutate while the first writer is live. The first settles before the guard is released; the second re-observes and rejects stale/dirty state as appropriate. Boundary cancellation keeps exactly the committed prefix.                                                                                                                                                                                                                                                                                                 |
| Zero-work path                       | Pass already-observed `Ready(target)` with panic-on-access registry and session fakes, plus a cold closure that panics if entered. Separately pass `RecoveryRequired` and request an explicit upgrade or downgrade to the observed version.                                                                                                                                                                                                                                                                                                                                                                                                                                          | Ordinary access returns `UpToDate` with no invocation, allocation, I/O or event; dirty is never accepted as current. Explicit requests fail with `WrongDirection` and no writes. Check allocation/event counts outside timing. Compare the optimized access gate against bare `Version` equality with runtime inputs and consumed outputs (`black_box`); report median/spread and measure adapter I/O separately.                                                                                                                               |

```rust
// Future fixture test. The panic fakes also fail on metadata, observation
// and capability reads, not only on writes.
let catalog = PanicCatalog;
let store = PanicStore;
let ready = StoreState::Ready(PINS_V2);
let result = gate_access(ready, PINS_V2, |_| check_fixture_compatibility(&catalog, &store));
assert!(matches!(result, Ok(Check::UpToDate)));

index.faults().kill_after_first_shard_removal();
let child = index.plan_and_run_in_subprocess(
    WITHOUT_INDEX, Direction::Upgrade, allow_derivable_non_atomic,
)?;
assert!(child.was_interrupted()); // The child observes and plans locally.
assert_eq!(index.read_marker()?, WITH_INDEX);
assert!(matches!(index.observe_fixture()?, GuardedObservation::RecoveryRequired));
index.recover_under_guard()?;
assert_eq!(index.shard_bytes()?, expected_shards);
```

Fault injection must include sync and ambiguous-outcome branches. Process-kill tests do not simulate power loss; document the durability assumptions of the supported fixture backend and use simulated persistence failures to test classification. Do not generalize those results to real shared/network filesystems or all platforms.

## Alternatives, risks and open questions

- **Adapter-only migrations** avoid a premature common API but duplicate path and policy validation. Keep this as the fallback for execution: if either fixture requires storage-specific orchestration, misleading outcomes, or different guard ownership, stop and bring the findings back. Reconsider sharing only the deterministic planner rather than adding store-specific hooks to the runner.
- **A graph or dynamically loaded steps** allow alternate paths but multiply preflight, downgrade and testing cases. There is no need for either; a validated slice resolves order without numeric adjacency.
- **One transaction across a chain** would hide the committed prefix, but needs uniform snapshot/restore semantics neither adapter can claim. Keep per-step guarantees and a clear failure boundary. **Automatic downgrade on open** is rejected because versions may not map, older binaries may not coordinate, and meaningful data may not fit.
- **F06.3 legacy promotion** would require proving semantic equivalence under new key inputs and policy. A cold start costs cache hits temporarily but avoids wrong hits. **F08 layout bump** would refuse some mixed-version peers, but is not a migration/exclusion protocol for already-running or older binaries. Keeping `VERSION` leaves the documented mixed-version lease/prune gap; no compatibility improvement is claimed.
- **Design ambiguity to review:** does every useful store have a cheap, coherent stale-plan token and a stable capability declaration independent of later intermediate data? The proposed answer for fixtures is explicit revision tracking and conservative capability declarations, with data checks at the affected step. The prototype owner and reviewer decide whether evidence supports a shared runner; not a prerequisite for starting the proof. Exact platform configuration syntax is owned by F06.3, and live-store quiescence/power-loss guarantees by any future adapter proposal. None is settled by these sketches.

The highest risks are false `Committed` after an ambiguous write (severe data loss), allowing a stale plan to write (severe lost edits), and releasing coordination under a running writer (severe concurrent corruption). Those are plausible with ordinary I/O failure or cancellation and are the first prototype checks. Residual risks include uncooperative writers, filesystem guarantees, and data-dependent refusal after a committed prefix; the first two bar production integration and the third is part of the contract.

## Ordered slices, checks and undo

| Slice / status                                                                           | Dependency and bounded result                                                                                                                                                                                                                                          | Verification and undo                                                                                                                                                                                                                               |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0. RFC only / in review when opened                                                      | Land the contract, store audit, reference sketches and acceptance cases into `main`, without the F07 branch or implementation commits.                                                                                                                                 | Review the rendered docs and `main`-relative paths/commits; run scoped Markdown formatting and whitespace checks. Revert the docs commit to withdraw the plan.                                                                                      |
| 1. Contract proof / blocked on slice 0 green, independent review and maintainer approval | Prototype the smallest planner/runner and both executable fixtures together in a test-only harness. First prove guarded observation, commit classification, competing writers and interruptions in both; add missing path, policy and refusal cases in the same slice. | Run the worked acceptance cases through both adapters, including subprocess races and reopen/recovery. If they contradict the generic contract, stop and revise the RFC before extracting anything. Remove the harness and disposable data to undo. |
| 2. Unwired leaf crate / blocked on slice 1 evidence and contract review                  | Extract the proven contract into `omni_migration`, with both fixtures as tests only. No Omni runtime dependents and no changes to existing stores.                                                                                                                     | Run crate format, Clippy and tests per [CONTRIBUTING](../../CONTRIBUTING.md); confirm dependency direction and unchanged production callers. Revert crate/workspace registration together.                                                          |
| 3. Performance and integration evidence / blocked on slice 2                             | Pin the fast-path regression test and benchmark and record fixture I/O separately; review the combined implementation before accepting it.                                                                                                                             | Run the no-access test, comparison benchmark, fault matrix and broad repository checks once at integration (CI-only targets stay in CI). Remove test/benchmark harness or revert the unwired crate if the contract is not supported.                |

The end state is reviewed internal infrastructure with repeatable evidence, not a rollout: no legacy path is switched or removed, no production marker is changed, and there is no new user command. Before any production adoption at or after v1, require a separate design and release plan covering mixed binaries, backups/recovery, compatibility and rollback. When this work finishes, distill durable decisions into the owning docs and API contracts; do not cite this plan as proof that the sketches ran.
