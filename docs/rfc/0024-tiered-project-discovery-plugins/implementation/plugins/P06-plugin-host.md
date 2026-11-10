# P06: `omni_plugin_host`

**Size:** XL (split) · **Depends on:** P03, P05 · **Unblocks:** P07, P08, P10, P11, P13

## Goal

The scheduler and enforcement layer between the contract and any handler: stage
snapshots, root/workspace partitions, hook-set validation, dispositions, dependency
ordering, claims and queries, atomic commits, budgets and cancellation. It accepts a
registry supplied by the composition root and imports no concrete builtins or engines.

## RFC references

- [§6.1 lifecycle, dispositions, partitions, transactions](../../006.00-hooks-and-lifecycle.md#61-hook-namespaces-and-lifecycle)
- [§6.1 claims and queries](../../006.01-claims-and-queries.md)
- [§8.1 shared safeguards](../../008.00-performance-and-shared-safeguards.md#81-shared-safeguards), [§8.4](../../008.03-tier-specific-enforcement.md)
- [§12.6 hook dispatch](../../012.00-rust-trait-sketches.md)

## Slices

1. **Activation graph:** resolve enabled descriptors, effective ids, producer bindings
   (canonical-name default, explicit id), graph validation (missing, invalid,
   cycles) before any hook. No implicit enabling or fetching.
2. **Negotiation + hook-set validation** (P03) before scheduling.
3. **Stage engine:** frozen stage-start snapshots, root assigned to at most one
   invocation per activation/stage, bounded batches (by count and bytes), worker pool,
   cancellation. Atomic stage commit: all-or-nothing state, contributions, notices.
4. **Dispositions:** transition table in §6.1; exactly one result per assigned root;
   reject missing/duplicate/unassigned; declined roots may still publish bounded
   supporting claims.
5. **Dependency-ordered inference:** producers finish and validate before consumers; the
   consumer's frozen producer-claim view is unchanged across retries; provisional
   producer output invisible on consumer failure.
6. **Claims and queries:** published/consumed declarations as grants, scoped bounded
   queries with paging, denied/unbound = error not empty, negative queries participate
   in snapshot validity.
7. **Resolution and observe stages:** resolution-only participation; observe is
   read-only over frozen views.
8. **Budgets:** host-wide and per-plugin limits; failed split/retry attempts consume
   budget; typed overflow classification enables size splitting; no retry for timeouts/
   traps.

## Acceptance

- Shuffled batch scheduling gives identical normalized results; cancellation and retries
  lose no state and duplicate no notices.
- Producer-before-consumer fixtures, missing/invalid bindings, cycles, aliased-id
  binding rules (no guessing), claim-schema incompatibility.
- Declined-root claims visible only via normal authorization; empty successful queries
  distinct from failures.
- Test with a mock in-process plugin only (native adapters come in P10/P11).
- No-plugin invocations do not initialize host or registry.
