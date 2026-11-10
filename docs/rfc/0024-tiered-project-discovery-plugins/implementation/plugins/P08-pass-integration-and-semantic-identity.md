# P08: Full-pass integration and execution identity

**Size:** L · **Depends on:** F06, F10, P04, P06, P07 · **Unblocks:** P10, P11, P12

## Goal

Wire the host into `Context::load_discovered_project_configs` and make the whole pass
consistent: composition, final-input revalidation, publication gating, and fine-grained
task-cache identity for inferred tasks.

## RFC references

- [§13.4 integration steps 1-5](../../013.00-implementation-plan.md#134-integration-points-in-current-plumbing)
- [§17.2 frozen claims vs inputs](../../017.00-review-findings-and-prerequisites.md#172-frozen-claims-do-not-imply-frozen-filesystem-inputs)
- [§8.3](../../008.02-execution-cache-semantic-identity.md#83-fine-grained-execution-cache-integration)
- [§16 walkthrough](../../016.00-native-plugin-walkthrough.md)

## Work

1. Pipeline: explicit layers → eligibility → inference hooks → freeze → dependency
   hooks → compose (activation layers, apply, defaults, conflicts) → resolve aliases and
   task inheritance → resolve edges and lower references → validate → observation →
   revalidate → publish.
2. Capture bootstrap inputs (workspace config and any layer supplying activation,
   selection, options, permission ceilings) in the F10 view.
3. Final revalidation: detected change **rejects** the pass (default). Bounded restart
   implemented and tested but left at zero retries; restart never carries rejected
   state; retries only for detected concurrent changes.
4. Rejected pass: no tasks run, no cache publication, no leaked notices, no stale
   fallback.
5. Inferred tasks use F05 fallback (uncached) and F06 semantic digest; plugin artifact
   fingerprints do **not** invalidate task execution when semantics are unchanged.
6. Lazy startup must not skip registered singleton hooks; required static checks still
   run when no markers match.

## Acceptance

- Mutation during reads, during cache-hit validation and at finalization; creation/
  deletion of previously missing files; marker/membership changes.
- Default rejection; restart success after one mutation (feature test-only); exhaustion
  under repeated mutation; cancellation; fresh per-attempt state.
- End-to-end with a mock plugin: discover → plan → run → warm rerun reuses inference and
  task cache correctly.
- Zero-plugin discovery equals baseline.

## Task P08.5: impact-relation selection (shared)

Task `P08.5` (deps P08.1, P06.5) implements the cycle-tolerant **impact relation** used
only for affected/dependent selection, never scheduling or cycle validation
([§4.3](../../004.01-native-ecosystem-adapters.md)):

- Transitive, cycle-safe fixed point over retained `package_dependency` claims
  (resolution `workspace`, no project edge) plus project edges.
- Conservative fallback when evidence for excluded roots cannot be established: treat
  selected projects related to any excluded root as impacted by a change under any
  excluded or selected project; fail with an actionable diagnostic if even that cannot
  be computed.
- Shared workspace manifest/lockfile changes select every selected member.
- Until it lands, dependent-expanding selection falls back to including projects with
  any retained workspace relationship to a changed project.
- Fixtures: direct, transitive dev chain, excluded `lib` between selected projects,
  excluded chain, explicit-only target, full workspace with nothing excluded.

Both adapters (P10.7, P11.5) depend on it.
