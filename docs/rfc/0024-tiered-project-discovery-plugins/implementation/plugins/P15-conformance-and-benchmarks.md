# P15: Conformance suite, budgets and benchmarks

**Size:** L · **Depends on:** P10, P11, P13

## Goal

Prove native/Wasm equivalence and meet the performance contract; replace candidate
budgets with calibrated defaults.

## RFC references

- [§8 performance is a contract](../../008.00-performance-and-shared-safeguards.md), [§8.4](../../008.03-tier-specific-enforcement.md)
- [§10 required tests](../../010.00-implementation-and-rollout.md)
- [§13.6 definition of done](../../013.00-implementation-plan.md#136-definition-of-done-for-v1)

## Work

1. **Shared conformance suite** driven by the P01 data fixtures: identical normalized
   output, provenance and validation outcomes for native and Wasm; any future JS runtime
   must pass the same suite.
2. **Behavioral suites:** hook ordering/batching invariance; fresh/reused instances;
   session-state round trips; parallel batches lose no state; required/optional
   negotiation; observation cannot mutate; cache invalidation (edits, new/deleted
   inputs, membership, options, host upgrades, activation); permission negatives;
   zero-plugin parity.
3. **Benchmarks** (`crates/omni_bench`): no plugins, unmatched markers, 1,000 projects,
   many enabled plugins, cold/warm caches, long-lived MCP invalidation, cache I/O
   costs, split attempts, pass retries.
4. **Calibrate** counters and publish finite default budgets with native and Wasm
   results; matched rerun for async per-invocation cost (RFC notes the prototype
   comparison was unmatched).
5. CI regression gates on the benchmarks.

## Acceptance

- Suites green in default and feature-reduced builds.
- Published benchmark table; defaults documented; budgets enforced in code.
